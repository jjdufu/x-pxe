//! 可注入的时钟：生产用系统时钟，测试用可拨动的手动时钟。

use std::fmt::Debug;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use xpxe_domain::Timestamp;

/// 时钟。
pub trait Clock: Send + Sync + Debug {
    /// 当前时间。
    fn now(&self) -> Timestamp;
}

/// 系统时钟。
#[derive(Debug, Clone, Copy, Default)]
pub struct WallTime;

impl Clock for WallTime {
    fn now(&self) -> Timestamp {
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_secs());
        Timestamp::from_unix_seconds(secs)
    }
}

/// 手动时钟（测试用）。
#[derive(Debug, Default)]
pub struct ManualTime {
    /// 当前秒数。
    secs: AtomicU64,
}

impl ManualTime {
    /// 从指定时间开始。
    #[must_use]
    pub const fn starting_at(at: Timestamp) -> Self {
        Self {
            secs: AtomicU64::new(at.unix_seconds()),
        }
    }

    /// 向前拨动。
    pub fn advance(&self, by: Duration) {
        let _previous = self.secs.fetch_add(by.as_secs(), Ordering::SeqCst);
    }
}

impl Clock for ManualTime {
    fn now(&self) -> Timestamp {
        Timestamp::from_unix_seconds(self.secs.load(Ordering::SeqCst))
    }
}
