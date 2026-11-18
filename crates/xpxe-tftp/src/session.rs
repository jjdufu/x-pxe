//! 单次读传输的协议状态机。
//!
//! 逻辑块号从 1 开始用 `u64` 计数，线上块号取低 16 位（超过 65535 块后回绕到 0，
//! 与 tftpd-hpa、iPXE、EDK2 的行为一致）。逻辑块 0 表示 OACK。

use std::sync::Arc;
use std::time::Duration;

use crate::packet::{ErrorCode, Packet};

/// RFC 1350 默认块大小。
pub const DEFAULT_BLKSIZE: usize = 512;
/// RFC 2348 允许的最小块大小。
const MIN_BLKSIZE: usize = 8;
/// 默认重传超时。
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(2);

/// 服务端的协商上限。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// 允许的最大块大小（默认按 MTU 1500 计算为 1468，见 [`Limits::for_mtu`]）。
    pub largest_block: usize,
    /// 允许的最大窗口。
    pub largest_window: u16,
    /// 同一窗口的最大重传次数。
    pub retries: u8,
}

impl Limits {
    /// 按网卡 MTU 计算块大小上限：MTU − 20（IP）− 8（UDP）− 4（TFTP 头），限定在 8–1468。
    #[must_use]
    pub fn for_mtu(mtu: usize) -> Self {
        Self {
            largest_block: mtu.saturating_sub(32).clamp(MIN_BLKSIZE, 1468),
            ..Self::default()
        }
    }
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            largest_block: 1468,
            largest_window: 16,
            retries: 5,
        }
    }
}

/// 状态机给传输层的指令。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// 依次发送这些报文，然后等待确认或超时。
    Send(Vec<Packet>),
    /// 报文无关（重复确认等），继续等待。
    Wait,
    /// 传输完成。
    Done,
    /// 放弃传输，可选地向对端发送错误。
    Abort(Option<Packet>),
}

/// 一次读传输。
#[derive(Debug, Clone)]
pub struct Session {
    /// 文件内容。
    data: Arc<[u8]>,
    /// 协商后的块大小。
    blksize: usize,
    /// 协商后的窗口。
    window: u16,
    /// 协商后的超时。
    timeout: Duration,
    /// 已协商的选项（为空表示不发 OACK）。
    oack: Vec<(String, String)>,
    /// 数据块总数（最后一块可能为空）。
    total: u64,
    /// 已被确认的最大逻辑块号。
    acked: u64,
    /// 已发送的最大逻辑块号。
    sent: u64,
    /// 当前窗口已重传次数。
    retries: u8,
    /// 重传上限。
    max_retries: u8,
}

/// 解析一个数值选项并限定在区间内；非法值返回 `None`（该选项被忽略）。
fn bounded(value: &str, low: u64, high: u64) -> Option<u64> {
    let parsed: u64 = value.trim().parse().ok()?;
    (parsed >= low).then(|| parsed.min(high))
}

/// 逻辑块号对应的线上块号。
fn wire_block(logical: u64) -> u16 {
    u16::try_from(logical & 0xFFFF).unwrap_or(0)
}

/// 协商中的参数。
struct Negotiated {
    /// 块大小。
    blksize: usize,
    /// 窗口。
    window: u16,
    /// 超时。
    timeout: Duration,
    /// 接受的选项。
    oack: Vec<(String, String)>,
}

impl Negotiated {
    /// 应用一个选项；非法值与未知选项被忽略（不出现在 OACK 中）。
    fn apply(&mut self, key: &str, value: &str, file_len: usize, limits: Limits) {
        let accepted = match key {
            "blksize" => {
                let cap = u64::try_from(limits.largest_block.max(MIN_BLKSIZE)).unwrap_or(u64::MAX);
                bounded(value, 8, cap).and_then(|size| {
                    self.blksize = usize::try_from(size).ok()?;
                    Some(size.to_string())
                })
            }
            "tsize" => Some(file_len.to_string()),
            "timeout" => bounded(value, 1, 255).map(|secs| {
                self.timeout = Duration::from_secs(secs);
                secs.to_string()
            }),
            "windowsize" => {
                bounded(value, 1, u64::from(limits.largest_window.max(1))).and_then(|size| {
                    self.window = u16::try_from(size).ok()?;
                    Some(size.to_string())
                })
            }
            _ => None,
        };
        if let Some(text) = accepted {
            self.oack.push((key.to_owned(), text));
        }
    }
}

impl Session {
    /// 根据读请求的选项建立会话。
    #[must_use]
    pub fn new(data: Arc<[u8]>, options: &[(String, String)], limits: Limits) -> Self {
        let mut negotiated = Negotiated {
            blksize: DEFAULT_BLKSIZE,
            window: 1,
            timeout: DEFAULT_TIMEOUT,
            oack: Vec::new(),
        };
        for (key, value) in options {
            negotiated.apply(key, value, data.len(), limits);
        }
        let Negotiated {
            blksize,
            window,
            timeout,
            oack,
        } = negotiated;
        let len = u64::try_from(data.len()).unwrap_or(u64::MAX);
        let blk = u64::try_from(blksize).unwrap_or(1).max(1);
        let total = len.checked_div(blk).unwrap_or(0).saturating_add(1);
        Self {
            data,
            blksize,
            window,
            timeout,
            oack,
            total,
            acked: 0,
            sent: 0,
            retries: 0,
            max_retries: limits.retries,
        }
    }

    /// 协商后的块大小。
    #[must_use]
    pub const fn blksize(&self) -> usize {
        self.blksize
    }

    /// 协商后的窗口。
    #[must_use]
    pub const fn window(&self) -> u16 {
        self.window
    }

    /// 协商后的重传超时。
    #[must_use]
    pub const fn timeout(&self) -> Duration {
        self.timeout
    }

    /// 已被确认的字节数。
    #[must_use]
    pub fn acked_bytes(&self) -> u64 {
        let blocks = usize::try_from(self.acked).unwrap_or(usize::MAX);
        let bytes = blocks.saturating_mul(self.blksize).min(self.data.len());
        u64::try_from(bytes).unwrap_or(u64::MAX)
    }

    /// 数据块总数。
    #[must_use]
    pub const fn total_blocks(&self) -> u64 {
        self.total
    }

    /// 第一批要发送的报文：有协商选项时为 OACK，否则为第一个窗口。
    #[must_use]
    pub fn start(&mut self) -> Action {
        if self.oack.is_empty() {
            Action::Send(self.window_from(1))
        } else {
            Action::Send(vec![Packet::OptionAck {
                options: self.oack.clone(),
            }])
        }
    }

    /// 取逻辑块的内容。
    fn block(&self, logical: u64) -> Packet {
        let index = usize::try_from(logical.saturating_sub(1)).unwrap_or(usize::MAX);
        let start = index.saturating_mul(self.blksize);
        let end = start.saturating_add(self.blksize).min(self.data.len());
        let data = self
            .data
            .get(start..end)
            .map_or_else(Vec::new, <[u8]>::to_vec);
        Packet::Data {
            block: wire_block(logical),
            data,
        }
    }

    /// 从某逻辑块开始发送一个窗口。
    fn window_from(&mut self, first: u64) -> Vec<Packet> {
        let last = first
            .saturating_add(u64::from(self.window))
            .saturating_sub(1)
            .min(self.total);
        self.sent = self.sent.max(last);
        (first..=last).map(|logical| self.block(logical)).collect()
    }

    /// 处理一个来自对端的报文。
    pub fn on_packet(&mut self, packet: &Packet) -> Action {
        match packet {
            Packet::Ack { block } => self.on_ack(*block),
            Packet::Error { .. } => Action::Abort(None),
            Packet::Read { .. }
            | Packet::Write
            | Packet::Data { .. }
            | Packet::OptionAck { .. } => Action::Abort(Some(Packet::Error {
                code: ErrorCode::IllegalOperation,
                message: "unexpected packet".to_owned(),
            })),
        }
    }

    /// 处理确认：在 `(acked, sent]` 中寻找线上块号匹配的逻辑块。
    fn on_ack(&mut self, block: u16) -> Action {
        let oack_pending = !self.oack.is_empty() && self.sent == 0;
        if oack_pending {
            if block != 0 {
                return Action::Wait;
            }
            self.retries = 0;
            return Action::Send(self.window_from(1));
        }
        let matched = (self.acked.saturating_add(1)..=self.sent)
            .rev()
            .find(|logical| wire_block(*logical) == block);
        let Some(logical) = matched else {
            return Action::Wait;
        };
        self.acked = logical;
        self.retries = 0;
        if self.acked >= self.total {
            return Action::Done;
        }
        Action::Send(self.window_from(self.acked.saturating_add(1)))
    }

    /// 超时：重传当前未确认的窗口，超过上限则放弃。
    pub fn on_timeout(&mut self) -> Action {
        if self.retries >= self.max_retries {
            return Action::Abort(None);
        }
        self.retries = self.retries.saturating_add(1);
        if !self.oack.is_empty() && self.sent == 0 {
            return self.start();
        }
        Action::Send(self.window_from(self.acked.saturating_add(1)))
    }
}

#[cfg(test)]
mod tests;
