//! `capture`：从 tcpdump 抓包文件（经典 pcap，以太网链路）中提取 DHCP 报文，
//! 原样保存为 `.bin` 并生成同名描述 `.json`。
//!
//! 实验室用 `tcpdump -i br-lab -w` 抓包、本子命令离线提取，避免探针自己开原始套接字
//! （门禁禁止 `unsafe`，原始套接字需要 libc）。

use std::collections::BTreeSet;
use std::net::Ipv4Addr;
use std::path::PathBuf;

use serde_json::json;
use xpxe_dhcp::wire::{Message, code};

use crate::common::{Args, LabError, LabResult, emit, format_mac};

/// 经典 pcap 微秒时间戳魔数（按小端读出）。
const MAGIC_USEC: u32 = 0xA1B2_C3D4;
/// 经典 pcap 纳秒时间戳魔数（按小端读出）。
const MAGIC_NSEC: u32 = 0xA1B2_3C4D;
/// 以太网链路类型。
const LINKTYPE_ETHERNET: u32 = 1;
/// 以太网头长度。
const ETHER_HEADER: usize = 14;
/// UDP 头长度。
const UDP_HEADER: usize = 8;
/// IPv4 以太类型。
const ETHERTYPE_IPV4: u16 = 0x0800;
/// 802.1Q 以太类型。
const ETHERTYPE_VLAN: u16 = 0x8100;
/// UDP 协议号。
const PROTO_UDP: u8 = 17;

/// 一个提取出的 DHCP 报文。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Captured {
    /// 抓包时间（秒）。
    pub(crate) seconds: u32,
    /// 源地址。
    pub(crate) src: Ipv4Addr,
    /// 目的地址。
    pub(crate) dst: Ipv4Addr,
    /// 源端口。
    pub(crate) sport: u16,
    /// 目的端口。
    pub(crate) dport: u16,
    /// UDP 负载（DHCP 报文）。
    pub(crate) payload: Vec<u8>,
}

/// 读取小端 `u32`。
fn le32(bytes: &[u8], at: usize) -> Option<u32> {
    let chunk = bytes.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes(<[u8; 4]>::try_from(chunk).ok()?))
}

/// 读取大端 `u16`。
fn be16(bytes: &[u8], at: usize) -> Option<u16> {
    let chunk = bytes.get(at..at.checked_add(2)?)?;
    Some(u16::from_be_bytes(<[u8; 2]>::try_from(chunk).ok()?))
}

/// 读取 IPv4 地址。
fn ipv4(bytes: &[u8], at: usize) -> Option<Ipv4Addr> {
    let chunk = bytes.get(at..at.checked_add(4)?)?;
    Some(Ipv4Addr::from(<[u8; 4]>::try_from(chunk).ok()?))
}

/// 从一帧以太网中取出 DHCP 相关端口的 UDP 负载。
fn frame(seconds: u32, data: &[u8]) -> Option<Captured> {
    let mut offset = ETHER_HEADER;
    let mut ethertype = be16(data, 12)?;
    if ethertype == ETHERTYPE_VLAN {
        ethertype = be16(data, 16)?;
        offset = offset.checked_add(4)?;
    }
    if ethertype != ETHERTYPE_IPV4 {
        return None;
    }
    let ip = data.get(offset..)?;
    let ihl = usize::from(ip.first()? & 0x0F).checked_mul(4)?;
    if *ip.get(9)? != PROTO_UDP {
        return None;
    }
    let total = usize::from(be16(ip, 2)?);
    let udp = ip.get(ihl..total.min(ip.len()))?;
    let sport = be16(udp, 0)?;
    let dport = be16(udp, 2)?;
    let ports = [67_u16, 68, 4011];
    if !ports.contains(&sport) && !ports.contains(&dport) {
        return None;
    }
    Some(Captured {
        seconds,
        src: ipv4(ip, 12)?,
        dst: ipv4(ip, 16)?,
        sport,
        dport,
        payload: udp.get(UDP_HEADER..)?.to_vec(),
    })
}

/// 解析整个 pcap 文件。
///
/// # Errors
///
/// 文件头不是以太网经典 pcap 时返回 [`LabError`]。
pub(crate) fn parse_pcap(bytes: &[u8]) -> LabResult<Vec<Captured>> {
    let magic = le32(bytes, 0).ok_or_else(|| LabError("pcap 过短".to_owned()))?;
    if magic != MAGIC_USEC && magic != MAGIC_NSEC {
        return Err(LabError(format!(
            "不支持的 pcap 魔数 {magic:08x}（需小端经典 pcap）"
        )));
    }
    if le32(bytes, 20) != Some(LINKTYPE_ETHERNET) {
        return Err(LabError("只支持以太网链路类型".to_owned()));
    }
    let mut out = Vec::new();
    let mut at = 24_usize;
    while let (Some(seconds), Some(included)) = (le32(bytes, at), le32(bytes, at.saturating_add(8)))
    {
        let start = at.saturating_add(16);
        let len = usize::try_from(included)
            .map_err(|overflow| LabError(format!("记录长度溢出：{overflow}")))?;
        let end = start.saturating_add(len);
        let Some(data) = bytes.get(start..end) else {
            break;
        };
        if let Some(captured) = frame(seconds, data) {
            out.push(captured);
        }
        at = end;
    }
    Ok(out)
}

/// 报文阶段名：`<消息类型>-<方向端口>`。
fn phase(captured: &Captured, message: &Message) -> String {
    let kind = message.message_type().map_or_else(
        || "bootp".to_owned(),
        |value| format!("{value:?}").to_lowercase(),
    );
    format!("{kind}-{}", captured.dport)
}

/// 子命令入口：`--pcap <文件> --out <目录> --label <前缀> [--clients-only] [--first-per-phase]`。
///
/// # Errors
///
/// 读写失败时返回 [`LabError`]。
pub(crate) fn run(args: &Args) -> LabResult {
    let pcap = std::fs::read(args.required("--pcap")?)?;
    let out = PathBuf::from(args.required("--out")?);
    let label = args.required("--label")?;
    let clients_only = args.flag("--clients-only");
    let first_only = args.flag("--first-per-phase");
    let allowed: BTreeSet<String> = args.values("--mac").into_iter().collect();
    std::fs::create_dir_all(&out)?;
    let mut seen = BTreeSet::new();
    let mut written = 0_usize;
    for captured in parse_pcap(&pcap)? {
        let Ok(message) = Message::decode(&captured.payload) else {
            continue;
        };
        let from_client = message.op == xpxe_dhcp::wire::Op::Request;
        if clients_only && !from_client {
            continue;
        }
        let mac = message.mac().map_or_else(String::new, format_mac);
        if !allowed.is_empty() && !allowed.contains(&mac) {
            continue;
        }
        let ipxe = message.is_ipxe();
        let stage = format!(
            "{}{}",
            phase(&captured, &message),
            if ipxe { "-ipxe" } else { "" }
        );
        if first_only && !seen.insert(stage.clone()) {
            continue;
        }
        written = written.saturating_add(1);
        let name = format!("{label}-{written:02}-{stage}");
        std::fs::write(out.join(format!("{name}.bin")), &captured.payload)?;
        let description = json!({
            "label": label,
            "stage": stage,
            "time": captured.seconds,
            "src": format!("{}:{}", captured.src, captured.sport),
            "dst": format!("{}:{}", captured.dst, captured.dport),
            "op": format!("{:?}", message.op),
            "message_type": format!("{:?}", message.message_type()),
            "mac": mac,
            "xid": format!("{:08x}", message.xid),
            "arch": message.client_arch(),
            "vendor_class": message.vendor_class(),
            "user_class": message.user_class(),
            "opt175": message.option(code::IPXE_ENCAP).is_some(),
            "is_ipxe": ipxe,
            "siaddr": message.siaddr.to_string(),
            "yiaddr": message.yiaddr.to_string(),
            "file": String::from_utf8_lossy(&message.file).trim_end_matches('\0').to_owned(),
            "options": message.options.iter().map(|(option, _value)| *option).collect::<Vec<u8>>(),
        });
        std::fs::write(
            out.join(format!("{name}.json")),
            format!("{}\n", serde_json::to_string_pretty(&description)?),
        )?;
        emit(&json!({"event": "fixture", "name": name}))?;
    }
    emit(&json!({"event": "done", "written": written}))
}

#[cfg(test)]
mod tests;
