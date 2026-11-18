//! 现网 DHCP 探测：构造探测 DISCOVER、分类收到的 OFFER。
//!
//! 只发 DISCOVER、不发 REQUEST，不占用租约。I/O 由 `xpxe-bootnet` 完成。

use std::net::Ipv4Addr;

use xpxe_domain::{Responder, ResponderClass};

use crate::wire::{Message, MessageType, Op, code};

/// 探测用的厂商类别：模拟 BIOS PXE ROM。
const PROBE_VENDOR: &[u8] = b"PXEClient:Arch:00000:UNDI:002001";

/// 构造带 `PXEClient` 厂商类的探测 DISCOVER（广播标志置位）。
///
/// `mac` 应为随机的本地管理地址（首字节第 2 位置 1、第 1 位清 0）。
#[must_use]
pub fn discover_packet(mac: [u8; 6], xid: u32) -> Message {
    let mut chaddr = [0_u8; 16];
    for (slot, byte) in chaddr.iter_mut().zip(mac) {
        *slot = byte;
    }
    Message {
        op: Op::Request,
        htype: 1,
        hlen: 6,
        hops: 0,
        xid,
        secs: 0,
        flags: 0x8000,
        ciaddr: Ipv4Addr::UNSPECIFIED,
        yiaddr: Ipv4Addr::UNSPECIFIED,
        siaddr: Ipv4Addr::UNSPECIFIED,
        giaddr: Ipv4Addr::UNSPECIFIED,
        chaddr,
        sname: Vec::new(),
        file: Vec::new(),
        options: vec![
            (code::MESSAGE_TYPE, vec![MessageType::Discover.to_byte()]),
            (code::PARAMETER_LIST, vec![1, 3, 6, 43, 54, 60, 66, 67]),
            (code::VENDOR_CLASS, PROBE_VENDOR.to_vec()),
            (code::CLIENT_ARCH, vec![0, 0]),
        ],
    }
}

/// 把 `seed` 变成本地管理的单播 MAC。
#[must_use]
pub const fn local_admin_mac(seed: [u8; 6]) -> [u8; 6] {
    let [first, rest @ ..] = seed;
    let [b1, b2, b3, b4, b5] = rest;
    [(first | 0x02) & 0xFE, b1, b2, b3, b4, b5]
}

/// `file` 字段（去掉 NUL 填充）或选项 67 的文本。
fn boot_file(reply: &Message) -> Option<String> {
    let field: Vec<u8> = reply
        .file
        .iter()
        .copied()
        .take_while(|byte| *byte != 0)
        .collect();
    if field.is_empty() {
        reply.option(code::BOOTFILE).map(|value| {
            String::from_utf8_lossy(value)
                .trim_end_matches('\0')
                .to_owned()
        })
    } else {
        Some(String::from_utf8_lossy(&field).into_owned())
    }
}

/// 分类一个应答；不是本次探测（`xid` 不符）的 OFFER 返回 `None`。
///
/// - `proxy`：`yiaddr = 0` 且选项 60 以 `PXEClient` 开头；
/// - `pxe_capable`：携带 `file`、选项 66/67，或选项 43 与 `PXEClient` 厂商类同时出现；
/// - 其余为 `address_only`。只有 `siaddr` 非零不算下发引导参数（dnsmasq 等默认把自己写进 `siaddr`）。
#[must_use]
pub fn classify(reply: &Message, xid: u32, source: Ipv4Addr) -> Option<Responder> {
    if reply.op != Op::Reply || reply.xid != xid || reply.message_type() != Some(MessageType::Offer)
    {
        return None;
    }
    let server = reply
        .option(code::SERVER_ID)
        .and_then(|value| <[u8; 4]>::try_from(value).ok())
        .map_or(source, Ipv4Addr::from);
    let pxe_vendor = reply
        .vendor_class()
        .is_some_and(|vendor| vendor.starts_with("PXEClient"));
    let file = boot_file(reply);
    let class = if reply.yiaddr == Ipv4Addr::UNSPECIFIED && pxe_vendor {
        ResponderClass::Proxy
    } else if file.is_some()
        || reply.option(code::TFTP_SERVER).is_some()
        || (pxe_vendor && reply.option(code::VENDOR_SPECIFIC).is_some())
    {
        ResponderClass::PxeCapable
    } else {
        ResponderClass::AddressOnly
    };
    Some(Responder {
        server,
        class,
        siaddr: reply.siaddr,
        file,
        options: reply.options.iter().map(|(option, _)| *option).collect(),
    })
}

#[cfg(test)]
mod tests;
