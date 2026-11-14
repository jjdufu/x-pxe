//! `proxydhcp`：最小 `ProxyDHCP`，只应答允许列表中的 MAC。
//!
//! 67 端口回应 PXE/HTTP/iPXE 的 DISCOVER（以及发给本机的 REQUEST），4011 端口回应 REQUEST。
//! 引导决策：iPXE（Opt77=`iPXE` 或带 Opt175）→ HTTP 脚本；Opt93=16 → HTTP Boot；
//! Opt93=0 → `undionly.kpxe`；其余 UEFI → `ipxe.efi`（可按 MAC 改为 `snponly.efi`）。

use std::collections::BTreeMap;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;

use serde_json::json;
use tokio::net::UdpSocket;
use xpxe_dhcp::proxy::{BootTarget, ReplyKind, build_reply};
use xpxe_dhcp::wire::{MAX_PACKET_LEN, Message, MessageType};

use crate::common::{
    Args, LabError, LabResult, bound_udp, emit, ensure_lab_iface, format_mac, parse_mac,
};

/// UEFI HTTP Boot 的客户端架构（x64）。
const ARCH_HTTP_X64: u16 = 16;
/// BIOS 客户端架构。
const ARCH_BIOS: u16 = 0;

/// `ProxyDHCP` 配置。
#[derive(Debug, Clone)]
pub(crate) struct Config {
    /// 本机在实验网上的地址。
    pub(crate) server: Ipv4Addr,
    /// 允许应答的 MAC。
    pub(crate) allow: Vec<[u8; 6]>,
    /// HTTP 根地址（含端口）。
    pub(crate) boot_base: String,
    /// UEFI PXE 默认固件文件。
    pub(crate) efi_file: String,
    /// 按 MAC 指定的 UEFI 固件文件。
    pub(crate) efi_override: BTreeMap<[u8; 6], String>,
    /// BIOS PXE 固件文件。
    pub(crate) bios_file: String,
}

/// 决策结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Decision {
    /// 不应答，附原因。
    Ignore(&'static str),
    /// 按目标应答。
    Reply(ReplyKind, BootTarget),
}

/// 根据请求与入站端口做引导决策（纯函数，单元测试覆盖）。
pub(crate) fn decide(config: &Config, request: &Message, port: u16) -> Decision {
    let Some(mac) = request.mac() else {
        return Decision::Ignore("no-mac");
    };
    let vendor = request.vendor_class().unwrap_or_default();
    let ipxe = request.is_ipxe();
    if !ipxe && !vendor.starts_with("PXEClient") && !vendor.starts_with("HTTPClient") {
        return Decision::Ignore("not-pxe");
    }
    if !config.allow.contains(&mac) {
        return Decision::Ignore("mac-not-allowed");
    }
    let kind = match (request.message_type(), port) {
        (Some(MessageType::Discover), 67) => ReplyKind::Offer,
        (Some(MessageType::Request), 4011) => ReplyKind::Ack,
        (Some(MessageType::Request), _)
            if request.option(xpxe_dhcp::wire::code::SERVER_ID)
                == Some(config.server.octets().as_slice()) =>
        {
            ReplyKind::Ack
        }
        _ => return Decision::Ignore("message-type"),
    };
    let target = if ipxe {
        BootTarget::IpxeScript {
            url: format!("{}/boot.ipxe", config.boot_base),
        }
    } else {
        match request.client_arch() {
            Some(ARCH_HTTP_X64) => BootTarget::HttpBoot {
                url: format!("{}/fw/x64/ipxe.efi", config.boot_base),
            },
            Some(ARCH_BIOS) | None => BootTarget::Tftp {
                server: config.server,
                file: config.bios_file.clone(),
            },
            Some(_) => BootTarget::Tftp {
                server: config.server,
                file: config
                    .efi_override
                    .get(&mac)
                    .cloned()
                    .unwrap_or_else(|| config.efi_file.clone()),
            },
        }
    };
    Decision::Reply(kind, target)
}

/// 从参数构造配置。
///
/// # Errors
///
/// 参数缺失或非法时返回 [`LabError`]。
fn config(args: &Args) -> LabResult<Config> {
    let allow = args
        .values("--allow-mac")
        .iter()
        .map(|text| parse_mac(text))
        .collect::<LabResult<Vec<_>>>()?;
    let mut efi_override = BTreeMap::new();
    for item in args.values("--efi-file-for") {
        let (mac, file) = item
            .split_once('=')
            .ok_or_else(|| LabError(format!("--efi-file-for 需为 <mac>=<file>：{item}")))?;
        let _previous = efi_override.insert(parse_mac(mac)?, file.to_owned());
    }
    Ok(Config {
        server: args.parsed("--server-ip", Ipv4Addr::new(10, 77, 0, 1))?,
        allow,
        boot_base: args.required("--boot-base")?,
        efi_file: args
            .value("--efi-file")
            .unwrap_or_else(|| "ipxe.efi".to_owned()),
        efi_override,
        bios_file: args
            .value("--bios-file")
            .unwrap_or_else(|| "undionly.kpxe".to_owned()),
    })
}

/// 处理单个入站报文。
///
/// # Errors
///
/// 编码或发送失败时返回 [`LabError`]。
async fn handle(
    config: &Config,
    socket: &UdpSocket,
    port: u16,
    payload: &[u8],
    from: SocketAddr,
) -> LabResult {
    let Ok(request) = Message::decode(payload) else {
        return Ok(());
    };
    if request.op != xpxe_dhcp::wire::Op::Request {
        return Ok(());
    }
    let mac = request.mac().map_or_else(String::new, format_mac);
    let common = json!({
        "port": port,
        "from": from.to_string(),
        "mac": mac,
        "xid": format!("{:08x}", request.xid),
        "message_type": format!("{:?}", request.message_type()),
        "arch": request.client_arch(),
        "vendor_class": request.vendor_class(),
        "user_class": request.user_class(),
        "ipxe": request.is_ipxe(),
        "opt175": request.option(xpxe_dhcp::wire::code::IPXE_ENCAP).is_some(),
    });
    match decide(config, &request, port) {
        Decision::Ignore(reason) => {
            emit(&json!({"event": "ignore", "reason": reason, "request": common}))
        }
        Decision::Reply(kind, target) => {
            let reply = build_reply(&request, config.server, kind, &target);
            let destination = if port == 67 {
                SocketAddr::from((Ipv4Addr::BROADCAST, 68))
            } else {
                from
            };
            let _sent = socket.send_to(&reply.encode()?, destination).await?;
            emit(&json!({
                "event": "reply",
                "kind": format!("{kind:?}"),
                "target": format!("{target:?}"),
                "to": destination.to_string(),
                "request": common,
            }))
        }
    }
}

/// 在一个端口上循环服务。
///
/// # Errors
///
/// 接收失败时返回 [`LabError`]。
async fn serve_port(config: Arc<Config>, socket: UdpSocket, port: u16) -> LabResult {
    let mut buf = vec![0_u8; MAX_PACKET_LEN];
    loop {
        let (len, from) = socket.recv_from(&mut buf).await?;
        let payload = buf.get(..len).unwrap_or_default();
        if let Err(error) = handle(&config, &socket, port, payload, from).await {
            emit(&json!({"event": "error", "detail": error.to_string()}))?;
        }
    }
}

/// 子命令入口。
///
/// # Errors
///
/// 网卡守卫、参数或套接字失败时返回 [`LabError`]。
pub(crate) fn run(args: &Args) -> LabResult {
    let iface = args.required("--iface")?;
    ensure_lab_iface(&iface)?;
    let config = Arc::new(config(args)?);
    let runtime = crate::common::runtime()?;
    runtime.block_on(async move {
        let dhcp = bound_udp(&iface, Ipv4Addr::UNSPECIFIED, 67)?;
        let pxe = bound_udp(&iface, Ipv4Addr::UNSPECIFIED, 4011)?;
        emit(&json!({"event": "listening", "iface": iface, "server": config.server.to_string(), "ports": [67_u16, 4011_u16]}))?;
        let other = tokio::spawn(serve_port(Arc::clone(&config), pxe, 4011));
        let result = serve_port(config, dhcp, 67).await;
        other.abort();
        result
    })
}

#[cfg(test)]
mod tests;
