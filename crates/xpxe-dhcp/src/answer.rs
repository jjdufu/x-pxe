//! 把领域决策（[`xpxe_domain::DhcpAnswer`]）编码为 `ProxyDHCP` 应答报文。
//!
//! 输入是请求报文与领域层给出的 [`BootOffer`]，输出是完整的应答 [`Message`] 与目的地址。
//! 是否应答、给哪个引导目标，全部由 `xpxe_domain::decision::answer` 决定，本模块只做编码。

use std::net::{Ipv4Addr, SocketAddrV4};

use xpxe_domain::{
    BootOffer, BootTarget as DomainTarget, DhcpRequestView, FirmwareSet, MacAddr, RequestSummary,
};

use crate::proxy::{BootTarget, ReplyKind, build_reply};
use crate::wire::{Message, MessageType, Op, code};

/// DHCP 客户端端口。
const CLIENT_PORT: u16 = 68;
/// BOOTP 广播标志位。
const BROADCAST_FLAG: u16 = 0x8000;

/// 编码应答所需的本机上下文。
#[derive(Debug, Clone, Copy)]
pub struct ReplyContext<'ctx> {
    /// 本机在该网卡上的地址（选项 54、`siaddr`）。
    pub server: Ipv4Addr,
    /// OFFER 或 ACK。
    pub kind: ReplyKind,
    /// 该节点的固件集（拼 `fw/<sha12>/<名称>`）。
    pub firmware: &'ctx FirmwareSet,
    /// 是否同时写选项 66/67。
    pub mirror_bootfile: bool,
}

/// 由领域要约构造应答报文。
#[must_use]
pub fn encode_offer(request: &Message, offer: &BootOffer, ctx: &ReplyContext<'_>) -> Message {
    let target = match &offer.target {
        DomainTarget::Tftp(name) => BootTarget::Tftp {
            server: ctx.server,
            file: ctx.firmware.tftp_path(*name),
        },
        DomainTarget::Http(url) if offer.http_client_class => BootTarget::HttpBoot {
            url: url.as_str().to_owned(),
        },
        DomainTarget::Http(url) => BootTarget::IpxeScript {
            url: url.as_str().to_owned(),
        },
    };
    let mut reply = build_reply(request, ctx.server, ctx.kind, &target);
    if ctx.mirror_bootfile {
        mirror_bootfile(&mut reply);
    }
    reply
}

/// 把 `siaddr`/`file` 镜像写入选项 66（点分地址文本）与 67（文件名）。
///
/// `siaddr` 为 0（HTTP Boot）时不写 66。
pub fn mirror_bootfile(reply: &mut Message) {
    if reply.siaddr != Ipv4Addr::UNSPECIFIED {
        reply
            .options
            .push((code::TFTP_SERVER, reply.siaddr.to_string().into_bytes()));
    }
    if !reply.file.is_empty() {
        reply.options.push((code::BOOTFILE, reply.file.clone()));
    }
}

/// 应答发往何处。
///
/// - 4011 端口的请求：单播回源地址与源端口。
/// - 67 端口：`ciaddr = 0` 或广播标志置位时发 `255.255.255.255:68`，否则单播 `ciaddr:68`。
#[must_use]
pub fn reply_destination(request: &Message, port: u16, from: SocketAddrV4) -> SocketAddrV4 {
    if port != 67 {
        return from;
    }
    let broadcast = request.ciaddr == Ipv4Addr::UNSPECIFIED || request.flags & BROADCAST_FLAG != 0;
    if broadcast {
        SocketAddrV4::new(Ipv4Addr::BROADCAST, CLIENT_PORT)
    } else {
        SocketAddrV4::new(request.ciaddr, CLIENT_PORT)
    }
}

/// 请求在此端口上应得到的应答类型；其余组合不应答。
///
/// 67 端口只回 DISCOVER；4011 端口只回 REQUEST（含部分固件发往 4011 的 INFORM 以外的类型均忽略）。
#[must_use]
pub fn reply_kind(request: &Message, port: u16) -> Option<ReplyKind> {
    if request.op != Op::Request {
        return None;
    }
    match (request.message_type(), port) {
        (Some(MessageType::Discover), 67) => Some(ReplyKind::Offer),
        (Some(MessageType::Request), 4011) => Some(ReplyKind::Ack),
        (Some(_) | None, _) => None,
    }
}

/// 提取领域决策所需的请求视图；没有以太网 MAC 时返回 `None`。
#[must_use]
pub fn request_view(request: &Message) -> Option<DhcpRequestView> {
    let mac = MacAddr::parse(&hex::encode(request.mac()?)).ok()?;
    Some(DhcpRequestView {
        mac,
        giaddr: request.giaddr,
        vendor_class: request.vendor_class(),
        user_class: request.user_class(),
        arch: request.client_arch(),
        has_opt175: request.option(code::IPXE_ENCAP).is_some(),
    })
}

/// 观测用请求摘要。
#[must_use]
pub fn request_summary(view: &DhcpRequestView, port: u16) -> RequestSummary {
    RequestSummary {
        mac: view.mac,
        port,
        arch: view.arch,
        vendor_class: view.vendor_class.clone(),
        user_class: view.user_class.clone(),
    }
}

#[cfg(test)]
mod tests;
