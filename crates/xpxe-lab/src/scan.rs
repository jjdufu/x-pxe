//! `dhcp-scan` 与 `pxe-client`：从 68 端口发出 DISCOVER，收集所有应答。
//!
//! 两者都只允许在隔离实验网卡上运行（见 [`crate::common::ensure_lab_iface`]）。

use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::time::Duration;

use ring::rand::SecureRandom as _;
use serde_json::json;
use tokio::net::UdpSocket;
use xpxe_dhcp::wire::{MAX_PACKET_LEN, Message, MessageType, Op, RawOption, code};

use crate::common::{
    Args, LabError, LabResult, bound_udp, emit, ensure_lab_iface, format_mac, parse_mac,
};

/// 客户端类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Client {
    /// 普通 DHCP 客户端（`dhcp-scan`）。
    Plain,
    /// BIOS PXE（Opt93=0，无 Opt77/175）。
    Legacy,
    /// UEFI PXE（Opt93=7）。
    Uefi,
    /// UEFI HTTP Boot（Opt93=16）。
    Http,
}

/// 生成随机字节。
///
/// # Errors
///
/// 系统随机源失败时返回 [`LabError`]。
fn random<const N: usize>() -> LabResult<[u8; N]> {
    let mut out = [0_u8; N];
    ring::rand::SystemRandom::new()
        .fill(&mut out)
        .map_err(|unspecified| LabError(format!("随机数失败：{unspecified}")))?;
    Ok(out)
}

/// 模拟客户端的身份。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Identity {
    /// 网卡 MAC。
    pub(crate) mac: [u8; 6],
    /// 事务号。
    pub(crate) xid: u32,
    /// 客户端 GUID（Opt97）。
    pub(crate) guid: [u8; 16],
    /// 客户端当前地址（REQUEST 用，DISCOVER 为 0）。
    pub(crate) address: Ipv4Addr,
}

/// 随机生成本地管理的单播 MAC。
///
/// # Errors
///
/// 系统随机源失败时返回 [`LabError`]。
fn random_mac() -> LabResult<[u8; 6]> {
    let mut generated = random::<6>()?;
    if let Some(first) = generated.first_mut() {
        *first = (*first & 0xFE) | 0x02;
    }
    Ok(generated)
}

/// 构造客户端 DISCOVER（或发往 4011 的 REQUEST）。
pub(crate) fn client_message(client: Client, kind: MessageType, identity: Identity) -> Message {
    let Identity {
        mac,
        xid,
        guid,
        address,
    } = identity;
    let mut hardware = [0_u8; 16];
    hardware
        .iter_mut()
        .zip(mac)
        .for_each(|(slot, byte)| *slot = byte);
    let mut options: Vec<RawOption> = vec![
        (code::MESSAGE_TYPE, vec![kind.to_byte()]),
        (code::MAX_MESSAGE_SIZE, 1472_u16.to_be_bytes().to_vec()),
    ];
    let (vendor, arch, ndi): (&[u8], u16, [u8; 3]) = match client {
        Client::Plain => (b"", 0, [0; 3]),
        Client::Legacy => (b"PXEClient:Arch:00000:UNDI:002001", 0, [1, 2, 1]),
        Client::Uefi => (b"PXEClient:Arch:00007:UNDI:003016", 7, [1, 3, 16]),
        Client::Http => (b"HTTPClient:Arch:00016:UNDI:003001", 16, [1, 3, 1]),
    };
    let mut guid_option = vec![0_u8];
    guid_option.extend_from_slice(&guid);
    if client == Client::Plain {
        options.push((code::PARAMETER_LIST, vec![1, 3, 6, 43, 60, 66, 67]));
    } else {
        options.push((
            code::PARAMETER_LIST,
            vec![1, 3, 6, 43, 60, 66, 67, 93, 94, 97, 128, 129, 130],
        ));
        options.push((code::CLIENT_ARCH, arch.to_be_bytes().to_vec()));
        options.push((code::CLIENT_NDI, ndi.to_vec()));
        options.push((code::CLIENT_UUID, guid_option));
        options.push((code::VENDOR_CLASS, vendor.to_vec()));
    }
    Message {
        op: Op::Request,
        htype: 1,
        hlen: 6,
        hops: 0,
        xid,
        secs: 4,
        flags: 0x8000,
        ciaddr: address,
        yiaddr: Ipv4Addr::UNSPECIFIED,
        siaddr: Ipv4Addr::UNSPECIFIED,
        giaddr: Ipv4Addr::UNSPECIFIED,
        chaddr: hardware,
        sname: Vec::new(),
        file: Vec::new(),
        options,
    }
}

/// 应答是否下发了引导参数（`file`、`siaddr`、Opt66/67/43）。
pub(crate) fn carries_boot(reply: &Message) -> bool {
    reply.file.iter().any(|byte| *byte != 0)
        || reply.siaddr != Ipv4Addr::UNSPECIFIED
        || reply.option(code::TFTP_SERVER).is_some()
        || reply.option(code::BOOTFILE).is_some()
}

/// 应答的描述 JSON。
fn describe(reply: &Message, from: SocketAddr) -> serde_json::Value {
    let text = |option: u8| {
        reply
            .option(option)
            .map(|value| String::from_utf8_lossy(value).into_owned())
    };
    json!({
        "from": from.to_string(),
        "message_type": format!("{:?}", reply.message_type()),
        "server_id": reply.option(code::SERVER_ID).and_then(|value| <[u8; 4]>::try_from(value).ok()).map(|octets| Ipv4Addr::from(octets).to_string()),
        "yiaddr": reply.yiaddr.to_string(),
        "siaddr": reply.siaddr.to_string(),
        "file": String::from_utf8_lossy(&reply.file).trim_end_matches('\0').to_owned(),
        "opt60": text(code::VENDOR_CLASS),
        "opt66": text(code::TFTP_SERVER),
        "opt67": text(code::BOOTFILE),
        "opt43": reply.option(code::VENDOR_SPECIFIC).map(hex::encode),
        "opt97": reply.option(code::CLIENT_UUID).map(hex::encode),
        "proxy": reply.yiaddr == Ipv4Addr::UNSPECIFIED,
        "boot_params": carries_boot(reply),
    })
}

/// 在超时内收集与 `xid` 匹配的应答。
///
/// # Errors
///
/// 接收失败时返回 [`LabError`]。
async fn collect(
    socket: &UdpSocket,
    xid: u32,
    wait: Duration,
) -> LabResult<Vec<(Message, SocketAddr, Vec<u8>)>> {
    let deadline = tokio::time::Instant::now()
        .checked_add(wait)
        .ok_or_else(|| LabError("等待时间溢出".to_owned()))?;
    let mut buf = vec![0_u8; MAX_PACKET_LEN];
    let mut out = Vec::new();
    while let Ok(received) = tokio::time::timeout_at(deadline, socket.recv_from(&mut buf)).await {
        let (len, from) = received?;
        let payload = buf.get(..len).unwrap_or_default();
        if let Ok(reply) = Message::decode(payload)
            && reply.op == Op::Reply
            && reply.xid == xid
        {
            out.push((reply, from, payload.to_vec()));
        }
    }
    Ok(out)
}

/// 保存报文到夹具目录。
///
/// # Errors
///
/// 写入失败时返回 [`LabError`]。
fn save(
    dir: Option<&PathBuf>,
    name: &str,
    payload: &[u8],
    description: &serde_json::Value,
) -> LabResult {
    let Some(root) = dir else {
        return Ok(());
    };
    std::fs::create_dir_all(root)?;
    std::fs::write(root.join(format!("{name}.bin")), payload)?;
    std::fs::write(
        root.join(format!("{name}.json")),
        format!("{}\n", serde_json::to_string_pretty(description)?),
    )?;
    Ok(())
}

/// 通用流程：发 DISCOVER、收应答；UEFI 模式下再向 `ProxyDHCP` 的 4011 发 REQUEST。
///
/// # Errors
///
/// 网卡守卫、参数或套接字失败时返回 [`LabError`]。
fn probe(args: &Args, client: Client) -> LabResult<Vec<serde_json::Value>> {
    let iface = args.required("--iface")?;
    ensure_lab_iface(&iface)?;
    let wait = Duration::from_secs(args.parsed("--wait", 3_u64)?);
    let mac = args
        .value("--mac")
        .map_or_else(random_mac, |text| parse_mac(&text))?;
    let address: Ipv4Addr = args.parsed("--ip", Ipv4Addr::UNSPECIFIED)?;
    let save_dir = args.value("--save").map(PathBuf::from);
    let label = args.value("--label").unwrap_or_else(|| "probe".to_owned());
    let xid = u32::from_be_bytes(random::<4>()?);
    let guid = random::<16>()?;
    let runtime = crate::common::runtime()?;
    runtime.block_on(async move {
        let socket = bound_udp(&iface, Ipv4Addr::UNSPECIFIED, 68)?;
        let identity = Identity { mac, xid, guid, address: Ipv4Addr::UNSPECIFIED };
        let discover = client_message(client, MessageType::Discover, identity);
        let encoded = discover.encode()?;
        let _sent = socket.send_to(&encoded, SocketAddr::from((Ipv4Addr::BROADCAST, 67))).await?;
        let sent_description = json!({"label": label, "stage": "discover-67", "constructed": true, "client": format!("{client:?}"), "mac": format_mac(mac)});
        save(save_dir.as_ref(), &format!("{label}-discover-67"), &encoded, &sent_description)?;
        let mut results = Vec::new();
        let replies = collect(&socket, xid, wait).await?;
        for (index, (reply, from, payload)) in replies.iter().enumerate() {
            let description = describe(reply, *from);
            save(save_dir.as_ref(), &format!("{label}-offer-{index}"), payload, &description)?;
            results.push(json!({"phase": "offer", "reply": description}));
        }
        let proxy = replies.iter().find(|(reply, _from, _payload)| {
            reply.yiaddr == Ipv4Addr::UNSPECIFIED
                && reply.option(code::VENDOR_CLASS).is_some_and(|value| value.starts_with(b"PXEClient"))
        });
        if client == Client::Uefi
            && address != Ipv4Addr::UNSPECIFIED
            && let Some((offer, from, _payload)) = proxy
        {
            let server = offer
                .option(code::SERVER_ID)
                .and_then(|value| <[u8; 4]>::try_from(value).ok())
                .map_or_else(|| from.ip(), |octets| Ipv4Addr::from(octets).into());
            let request = client_message(client, MessageType::Request, Identity { address, ..identity });
            let encoded_request = request.encode()?;
            let _sent4011 = socket.send_to(&encoded_request, SocketAddr::new(server, 4011)).await?;
            save(save_dir.as_ref(), &format!("{label}-request-4011"), &encoded_request, &json!({"label": label, "stage": "request-4011", "constructed": true}))?;
            for (reply, reply_from, payload) in collect(&socket, xid, wait).await? {
                let description = describe(&reply, reply_from);
                save(save_dir.as_ref(), &format!("{label}-ack-4011"), &payload, &description)?;
                results.push(json!({"phase": "ack-4011", "reply": description}));
            }
        }
        Ok(results)
    })
}

/// `dhcp-scan` 入口：列出所有应答的 DHCP 服务器，并标记下发引导参数者。
///
/// # Errors
///
/// 探测失败时返回 [`LabError`]。
pub(crate) fn run_scan(args: &Args) -> LabResult {
    let results = probe(args, Client::Plain)?;
    for result in &results {
        emit(result)?;
    }
    emit(
        &json!({"event": "summary", "servers": results.len(), "with_boot_params": results.iter().filter(|item| item.pointer("/reply/boot_params") == Some(&serde_json::Value::Bool(true))).count()}),
    )
}

/// `pxe-client` 入口：`--legacy|--uefi|--http`。
///
/// # Errors
///
/// 探测失败时返回 [`LabError`]。
pub(crate) fn run_pxe(args: &Args) -> LabResult {
    let client = if args.flag("--legacy") {
        Client::Legacy
    } else if args.flag("--http") {
        Client::Http
    } else if args.flag("--uefi") {
        Client::Uefi
    } else {
        return Err(LabError("需要 --legacy、--uefi 或 --http 之一".to_owned()));
    };
    let results = probe(args, client)?;
    for result in &results {
        emit(result)?;
    }
    emit(&json!({"event": "summary", "replies": results.len()}))
}

#[cfg(test)]
mod tests;
