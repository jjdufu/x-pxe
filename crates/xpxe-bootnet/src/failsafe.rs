//! 失败即关闭：连续 30 s 未成功拉取指令即清空指令表。

use std::time::{Duration, Instant};

use xpxe_domain::DirectiveSnapshot;

/// 失联判定阈值。
pub const STALE_AFTER: Duration = Duration::from_secs(30);

/// 最近一次成功与控制面通信的时间。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Liveness {
    /// 最近一次成功。
    last_ok: Instant,
}

impl Liveness {
    /// 从 `now` 开始计时。
    #[must_use]
    pub const fn new(now: Instant) -> Self {
        Self { last_ok: now }
    }

    /// 记录一次成功（200 或 304）。
    pub const fn mark_ok(&mut self, now: Instant) {
        self.last_ok = now;
    }

    /// 距上次成功是否已超过 [`STALE_AFTER`]。
    #[must_use]
    pub fn is_stale(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.last_ok) > STALE_AFTER
    }
}

/// 实际生效的快照：失联时保留模式与规则，但清空全部应答指令。
#[must_use]
pub fn effective(snapshot: &DirectiveSnapshot, stale: bool) -> DirectiveSnapshot {
    let mut copy = snapshot.clone();
    if stale {
        copy.directives.clear();
    }
    copy
}

#[cfg(test)]
mod tests;
