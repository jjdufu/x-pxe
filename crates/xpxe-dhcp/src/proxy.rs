//! `ProxyDHCP` 应答报文构造。
//!
//! `ProxyDHCP` 不分配地址（`yiaddr = 0`），只回引导信息；地址由现网 DHCP 分配。
//! 本模块只负责“给定引导目标，构造符合 PXE 2.1 / UEFI HTTP Boot 要求的报文”，
//! 是否应答由上层的引导决策决定。

use std::net::Ipv4Addr;

use crate::wire::{Message, MessageType, Op, code};

/// 引导目标。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BootTarget {
    /// 经 TFTP 加载固件（PXE ROM / EDK2 `PxeBc`）。
    Tftp {
        /// TFTP 服务器地址（写入 `siaddr`）。
        server: Ipv4Addr,
        /// 文件名（写入 `file`）。
        file: String,
    },
    /// UEFI HTTP Boot：`file` 写完整 URL，厂商类别回 `HTTPClient`。
    HttpBoot {
        /// 固件 URL，端口必须写全。
        url: String,
    },
    /// iPXE 客户端：`file` 写 iPXE 脚本 URL，`siaddr` 写本机地址。
    ///
    /// iPXE 只在 `siaddr` 非零且有引导文件名（或 PXE 启动菜单）时才接受 `ProxyDHCP` 应答
    /// （`dhcp_has_pxeopts`），因此即使走 HTTP 也必须填 `siaddr`（实验室 B3 抓包证实）。
    IpxeScript {
        /// 脚本 URL，端口必须写全。
        url: String,
    },
}

/// 应答类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplyKind {
    /// 回应 67 端口的 DISCOVER。
    Offer,
    /// 回应 4011 端口（或 67 端口）的 REQUEST。
    Ack,
}

/// PXE 厂商封装选项：子选项 6（`PXE_DISCOVERY_CONTROL`）= 0x08，表示跳过启动服务器发现，
/// 直接使用报文里的引导文件。
const PXE_VENDOR_OPTIONS: [u8; 4] = [6, 1, 8, 255];

/// 构造 `ProxyDHCP` 应答。
///
/// `server` 是本机在该网段上的地址（选项 54）。请求中的选项 97（客户端 GUID）原样回显，
/// 这是 EDK2 `PxeBc` 接受 `ProxyDHCP` 应答的条件之一。
#[must_use]
pub fn build_reply(
    request: &Message,
    server: Ipv4Addr,
    kind: ReplyKind,
    target: &BootTarget,
) -> Message {
    let message_type = match kind {
        ReplyKind::Offer => MessageType::Offer,
        ReplyKind::Ack => MessageType::Ack,
    };
    let (vendor_class, siaddr, file): (&[u8], Ipv4Addr, &str) = match target {
        BootTarget::Tftp { server: tftp, file } => (b"PXEClient", *tftp, file.as_str()),
        BootTarget::HttpBoot { url } => (b"HTTPClient", Ipv4Addr::UNSPECIFIED, url.as_str()),
        BootTarget::IpxeScript { url } => (b"PXEClient", server, url.as_str()),
    };
    let mut options = vec![
        (code::MESSAGE_TYPE, vec![message_type.to_byte()]),
        (code::SERVER_ID, server.octets().to_vec()),
        (code::VENDOR_CLASS, vendor_class.to_vec()),
    ];
    if let Some(guid) = request.option(code::CLIENT_UUID) {
        options.push((code::CLIENT_UUID, guid.to_vec()));
    }
    if matches!(target, BootTarget::Tftp { .. }) {
        options.push((code::VENDOR_SPECIFIC, PXE_VENDOR_OPTIONS.to_vec()));
    }
    Message {
        op: Op::Reply,
        htype: request.htype,
        hlen: request.hlen,
        hops: 0,
        xid: request.xid,
        secs: 0,
        flags: request.flags,
        ciaddr: request.ciaddr,
        yiaddr: Ipv4Addr::UNSPECIFIED,
        siaddr,
        giaddr: request.giaddr,
        chaddr: request.chaddr,
        sname: Vec::new(),
        file: file.as_bytes().to_vec(),
        options,
    }
}

#[cfg(test)]
mod tests;
