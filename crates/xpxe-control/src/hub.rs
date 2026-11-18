//! SSE 推送中心：广播 + 最近事件环形缓冲（断线重连按 `Last-Event-ID` 补发）。

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, PoisonError};

use serde::Serialize;
use serde_json::Value;
use tokio::sync::broadcast;

/// 环形缓冲容量。
const RECENT: usize = 512;

/// 一条推送。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct Pushed {
    /// 单调递增 ID。
    pub(crate) id: u64,
    /// 种类，例如 `node.state`、`node.event`、`task.progress`、`bootnet.health`、`config.version`。
    pub(crate) kind: String,
    /// 数据。
    pub(crate) data: Value,
}

/// 内部状态。
#[derive(Debug, Default)]
struct Ring {
    /// 下一个 ID。
    next: u64,
    /// 最近事件。
    recent: VecDeque<Arc<Pushed>>,
}

/// 订阅结果：补发列表与实时接收端。
pub(crate) type Subscription = (Vec<Arc<Pushed>>, broadcast::Receiver<Arc<Pushed>>);

/// 推送中心。
#[derive(Debug)]
pub(crate) struct Hub {
    /// 广播端。
    sender: broadcast::Sender<Arc<Pushed>>,
    /// 环形缓冲。
    ring: Mutex<Ring>,
}

impl Hub {
    /// 新建。
    pub(crate) fn new() -> Self {
        let (sender, _receiver) = broadcast::channel(256);
        Self {
            sender,
            ring: Mutex::new(Ring {
                next: 1,
                recent: VecDeque::new(),
            }),
        }
    }

    /// 发布一条推送。
    pub(crate) fn publish(&self, kind: &str, data: Value) {
        let mut ring = self.ring.lock().unwrap_or_else(PoisonError::into_inner);
        let pushed = Arc::new(Pushed {
            id: ring.next,
            kind: kind.to_owned(),
            data,
        });
        ring.next = ring.next.saturating_add(1);
        if ring.recent.len() >= RECENT {
            let _oldest = ring.recent.pop_front();
        }
        ring.recent.push_back(Arc::clone(&pushed));
        drop(ring);
        let _receivers = self.sender.send(pushed);
    }

    /// 订阅，同时返回 `last_id` 之后的缓冲事件（先订阅后取缓冲，避免漏发）。
    pub(crate) fn subscribe(&self, last_id: Option<u64>) -> Subscription {
        let receiver = self.sender.subscribe();
        let ring = self.ring.lock().unwrap_or_else(PoisonError::into_inner);
        let replay = last_id.map_or_else(Vec::new, |last| {
            ring.recent
                .iter()
                .filter(|pushed| pushed.id > last)
                .cloned()
                .collect()
        });
        drop(ring);
        (replay, receiver)
    }
}

#[cfg(test)]
mod tests;
