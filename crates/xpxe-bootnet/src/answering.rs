//! 单个 DHCP 报文的处理：限速 → 解码 → 领域决策 → 编码。纯逻辑，不做 I/O。

use std::collections::BTreeMap;
use std::net::{Ipv4Addr, SocketAddrV4};
use std::time::{Duration, Instant};

use xpxe_dhcp::answer::{
    ReplyContext, encode_offer, reply_destination, reply_kind, request_summary, request_view,
};
use xpxe_dhcp::proxy::ReplyKind;
use xpxe_dhcp::wire::{MAX_PACKET_LEN, Message};
use xpxe_domain::{
    BootTarget, DhcpAnswer, DirectiveSnapshot, IgnoreReason, MacAddr, Observation, ReplyStage,
    Timestamp, answer,
};

use crate::state::Counter;

/// 全局限速：每秒报文数。
const GLOBAL_PER_SECOND: u32 = 200;
/// 每 MAC 每分钟应答数。
const PER_MAC_PER_MINUTE: u32 = 20;
/// 每 MAC 窗口表超过此大小时清理过期项。
const PER_MAC_PRUNE_AT: usize = 4096;
/// 本进程探测用 MAC 的前缀（本地管理位已置）：`02:78:70:78:65`（"xpxe"）。
pub const PROBE_MAC_PREFIX: [u8; 5] = [0x02, 0x78, 0x70, 0x78, 0x65];

/// 固定窗口计数。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Window {
    /// 窗口起点。
    start: Instant,
    /// 窗口内计数。
    count: u32,
}

impl Window {
    /// 在窗口内计一次；超出 `limit` 返回 `false`。
    fn admit(&mut self, now: Instant, span: Duration, limit: u32) -> bool {
        if now.saturating_duration_since(self.start) >= span {
            self.start = now;
            self.count = 0;
        }
        if self.count >= limit {
            return false;
        }
        self.count = self.count.saturating_add(1);
        true
    }
}

/// 限速器：全局每秒 200 个报文、每 MAC 每分钟 20 个应答。
#[derive(Debug, Clone)]
pub struct RateLimiter {
    /// 全局窗口。
    global: Window,
    /// 每 MAC 窗口。
    per_mac: BTreeMap<MacAddr, Window>,
}

impl RateLimiter {
    /// 新建。
    #[must_use]
    pub const fn new(now: Instant) -> Self {
        Self {
            global: Window {
                start: now,
                count: 0,
            },
            per_mac: BTreeMap::new(),
        }
    }

    /// 全局报文配额。
    pub fn admit_packet(&mut self, now: Instant) -> bool {
        self.global
            .admit(now, Duration::from_secs(1), GLOBAL_PER_SECOND)
    }

    /// 该 MAC 的应答配额。
    pub fn admit_reply(&mut self, mac: MacAddr, now: Instant) -> bool {
        let minute = Duration::from_secs(60);
        if self.per_mac.len() >= PER_MAC_PRUNE_AT {
            self.per_mac
                .retain(|_mac, window| now.saturating_duration_since(window.start) < minute);
        }
        self.per_mac
            .entry(mac)
            .or_insert(Window {
                start: now,
                count: 0,
            })
            .admit(now, minute, PER_MAC_PER_MINUTE)
    }
}

/// 一个入站报文。
#[derive(Debug, Clone, Copy)]
pub struct Inbound<'pkt> {
    /// 报文字节。
    pub payload: &'pkt [u8],
    /// 入站端口（67 或 4011）。
    pub port: u16,
    /// 源地址。
    pub from: SocketAddrV4,
}

/// 处理时的环境。
#[derive(Debug, Clone, Copy)]
pub struct Context<'ctx> {
    /// 当前生效的快照。
    pub snapshot: &'ctx DirectiveSnapshot,
    /// 本机在该网卡上的地址。
    pub server: Ipv4Addr,
    /// 是否因冲突暂停应答。
    pub paused: bool,
    /// 单调时钟。
    pub now: Instant,
    /// 墙上时钟（判断指令过期）。
    pub wall: Timestamp,
}

/// 处理结论。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// 丢弃并计数（不写观测）。
    Dropped(Counter),
    /// 与本服务无关的报文（REQUEST/INFORM、非 PXE 客户端、自身探测），只计入 `ignored`。
    Unrelated,
    /// 不应答并写观测。
    Ignored(Box<Observation>),
    /// 发送应答。
    Replied {
        /// 应答字节。
        bytes: Vec<u8>,
        /// 目的地址。
        to: SocketAddrV4,
        /// 观测。
        observation: Box<Observation>,
    },
}

/// 领域忽略原因的稳定蛇形名。
#[must_use]
pub const fn reason_name(reason: IgnoreReason) -> &'static str {
    match reason {
        IgnoreReason::NotPxe => "not_pxe",
        IgnoreReason::Relayed => "relayed",
        IgnoreReason::NotArmed => "not_armed",
        IgnoreReason::Expired => "expired",
        IgnoreReason::UnsupportedArch => "unsupported_arch",
        IgnoreReason::HttpBootDisabled => "http_boot_disabled",
    }
}

/// 已解析的请求。
#[derive(Debug, Clone, Copy)]
struct Parsed<'req> {
    /// 报文。
    request: &'req Message,
    /// 应答类型。
    kind: ReplyKind,
    /// 决策视图。
    view: &'req xpxe_domain::DhcpRequestView,
}

/// DHCP 应答引擎。
#[derive(Debug, Clone)]
pub struct Engine {
    /// 限速器。
    limiter: RateLimiter,
}

impl Engine {
    /// 新建。
    #[must_use]
    pub const fn new(now: Instant) -> Self {
        Self {
            limiter: RateLimiter::new(now),
        }
    }

    /// 处理一个入站报文。
    pub fn handle(&mut self, inbound: Inbound<'_>, ctx: &Context<'_>) -> Verdict {
        if !self.limiter.admit_packet(ctx.now) {
            return Verdict::Dropped(Counter::DhcpRateLimited);
        }
        if inbound.payload.len() > MAX_PACKET_LEN {
            return Verdict::Dropped(Counter::DhcpMalformed);
        }
        let Ok(request) = Message::decode(inbound.payload) else {
            return Verdict::Dropped(Counter::DhcpMalformed);
        };
        let Some(kind) = reply_kind(&request, inbound.port) else {
            return Verdict::Unrelated;
        };
        let Some(view) = request_view(&request) else {
            return Verdict::Unrelated;
        };
        if view.mac.octets().starts_with(&PROBE_MAC_PREFIX) {
            return Verdict::Unrelated;
        }
        if ctx.paused {
            return Verdict::Dropped(Counter::DhcpPaused);
        }
        let parsed = Parsed {
            request: &request,
            kind,
            view: &view,
        };
        self.decide(&parsed, inbound, ctx)
    }

    /// 领域决策与编码。
    fn decide(&mut self, parsed: &Parsed<'_>, inbound: Inbound<'_>, ctx: &Context<'_>) -> Verdict {
        let Parsed {
            request,
            kind,
            view,
        } = *parsed;
        let summary = request_summary(view, inbound.port);
        let table = ctx.snapshot.table();
        let rules = ctx.snapshot.rules_for(view.mac);
        let offer = match answer(view, &table, rules, ctx.wall) {
            DhcpAnswer::Ignore(reason) if *reason == IgnoreReason::NotPxe => {
                return Verdict::Unrelated;
            }
            DhcpAnswer::Ignore(reason) => {
                return Verdict::Ignored(Box::new(Observation::DhcpIgnore {
                    at: ctx.wall,
                    request: summary,
                    reason: reason_name(*reason).to_owned(),
                }));
            }
            DhcpAnswer::Offer(offer) => offer,
        };
        let Some(entry) = ctx.snapshot.entry(view.mac) else {
            return Verdict::Unrelated;
        };
        if !self.limiter.admit_reply(view.mac, ctx.now) {
            return Verdict::Dropped(Counter::DhcpRateLimited);
        }
        let reply_ctx = ReplyContext {
            server: ctx.server,
            kind,
            firmware: &entry.firmware,
            mirror_bootfile: rules.mirror_bootfile_options(),
        };
        let reply = encode_offer(request, &offer, &reply_ctx);
        let Ok(bytes) = reply.encode() else {
            return Verdict::Dropped(Counter::DhcpMalformed);
        };
        let file = match &offer.target {
            BootTarget::Tftp(name) => entry.firmware.tftp_path(*name),
            BootTarget::Http(url) => url.as_str().to_owned(),
        };
        let stage = match kind {
            ReplyKind::Offer => ReplyStage::Offer,
            ReplyKind::Ack => ReplyStage::Ack,
        };
        Verdict::Replied {
            bytes,
            to: reply_destination(request, inbound.port, inbound.from),
            observation: Box::new(Observation::DhcpReply {
                at: ctx.wall,
                request: summary,
                stage,
                file,
            }),
        }
    }
}

#[cfg(test)]
mod tests;
