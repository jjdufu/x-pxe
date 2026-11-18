//! 进程内共享状态：计数器、待上报观测、健康状态与最近一次探测。

use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

use tokio::sync::Notify;
use xpxe_domain::{Counters, Health, Heartbeat, Observation, ProbeReport, Timestamp};

/// 观测队列上限：控制面长时间不可达时丢弃最旧的观测。
const QUEUE_LIMIT: usize = 10_000;
/// 满多少条立即上报。
pub const BATCH_SIZE: usize = 200;

/// 计数器种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Counter {
    /// 收到 DHCP 报文。
    DhcpReceived,
    /// 解析失败或超长。
    DhcpMalformed,
    /// 限速丢弃。
    DhcpRateLimited,
    /// 发出应答。
    DhcpReplies,
    /// 不应答。
    DhcpIgnored,
    /// 冲突暂停期间丢弃。
    DhcpPaused,
    /// TFTP 完成。
    TftpCompleted,
    /// TFTP 失败。
    TftpFailed,
}

/// 健康相关的可变状态。
#[derive(Debug, Default)]
struct Status {
    /// 已应用的快照版本。
    rev: u64,
    /// 当前模式名。
    mode: String,
    /// 正在监听的端口。
    listening: Vec<u16>,
    /// 当前问题（来源 → 说明）；非空即 `degraded`。
    problems: BTreeMap<&'static str, String>,
    /// 最近一次探测。
    probe: Option<ProbeReport>,
    /// 冲突暂停。
    paused: bool,
}

/// 共享状态。
#[derive(Debug, Default)]
pub struct Shared {
    /// 计数器。
    counters: Mutex<Counters>,
    /// 待上报观测。
    queue: Mutex<VecDeque<Observation>>,
    /// 健康状态。
    status: Mutex<Status>,
    /// 队列满一批时唤醒上报任务。
    batch_ready: Notify,
    /// 已请求停止。
    stopping: AtomicBool,
}

/// 取锁；被毒化时仍取回数据（状态只是计数与快照，不存在不变量被破坏的风险）。
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Shared {
    /// 后台循环是否应继续。
    #[must_use]
    pub fn running(&self) -> bool {
        !self.stopping.load(Ordering::Relaxed)
    }

    /// 请求后台循环停止。
    pub fn request_stop(&self) {
        self.stopping.store(true, Ordering::Relaxed);
        self.batch_ready.notify_one();
    }

    /// 计数加一。
    pub fn count(&self, counter: Counter) {
        let mut counters = lock(&self.counters);
        let slot = match counter {
            Counter::DhcpReceived => &mut counters.dhcp_received,
            Counter::DhcpMalformed => &mut counters.dhcp_malformed,
            Counter::DhcpRateLimited => &mut counters.dhcp_rate_limited,
            Counter::DhcpReplies => &mut counters.dhcp_replies,
            Counter::DhcpIgnored => &mut counters.dhcp_ignored,
            Counter::DhcpPaused => &mut counters.dhcp_paused,
            Counter::TftpCompleted => &mut counters.tftp_completed,
            Counter::TftpFailed => &mut counters.tftp_failed,
        };
        *slot = slot.saturating_add(1);
        drop(counters);
    }

    /// 当前计数。
    #[must_use]
    pub fn counters(&self) -> Counters {
        *lock(&self.counters)
    }

    /// 记录一条观测（同时写结构化日志）。
    pub fn observe(&self, observation: Observation) {
        match serde_json::to_string(&observation) {
            Ok(line) => {
                tracing::info!(target: "xpxe_bootnet::observation", observation = %line, "observation");
            }
            Err(error) => tracing::warn!(%error, "观测无法序列化"),
        }
        let mut queue = lock(&self.queue);
        if queue.len() >= QUEUE_LIMIT {
            let _oldest: Option<Observation> = queue.pop_front();
        }
        queue.push_back(observation);
        let full = queue.len() >= BATCH_SIZE;
        drop(queue);
        if full {
            self.batch_ready.notify_one();
        }
    }

    /// 取出至多一批观测。
    #[must_use]
    pub fn take_batch(&self) -> Vec<Observation> {
        let mut queue = lock(&self.queue);
        let count = queue.len().min(BATCH_SIZE);
        queue.drain(..count).collect()
    }

    /// 上报失败时把观测放回队首。
    pub fn requeue(&self, batch: Vec<Observation>) {
        let mut queue = lock(&self.queue);
        for observation in batch.into_iter().rev() {
            if queue.len() >= QUEUE_LIMIT {
                break;
            }
            queue.push_front(observation);
        }
    }

    /// 等待“满一批”的通知。
    pub async fn batch_ready(&self) {
        self.batch_ready.notified().await;
    }

    /// 设置一个问题（健康降为 `degraded`）。
    pub fn set_problem(&self, source: &'static str, detail: String) {
        let mut status = lock(&self.status);
        if status.problems.get(source) != Some(&detail) {
            tracing::warn!(source, detail = %detail, "bootnet degraded");
            let _previous: Option<String> = status.problems.insert(source, detail);
        }
    }

    /// 清除一个问题。
    pub fn clear_problem(&self, source: &'static str) {
        let mut status = lock(&self.status);
        if status.problems.remove(source).is_some() {
            tracing::info!(source, "bootnet problem cleared");
        }
    }

    /// 记录已应用的快照版本与模式。
    pub fn set_applied(&self, rev: u64, mode: &str) {
        let mut status = lock(&self.status);
        status.rev = rev;
        mode.clone_into(&mut status.mode);
    }

    /// 记录正在监听的端口。
    pub fn set_listening(&self, ports: Vec<u16>) {
        lock(&self.status).listening = ports;
    }

    /// 正在监听的端口。
    #[must_use]
    pub fn listening(&self) -> Vec<u16> {
        lock(&self.status).listening.clone()
    }

    /// 记录探测结论；返回是否因冲突暂停应答。
    pub fn set_probe(&self, report: ProbeReport, pause: bool) -> bool {
        let mut status = lock(&self.status);
        status.probe = Some(report);
        status.paused = pause;
        pause
    }

    /// 最近一次探测。
    #[must_use]
    pub fn probe(&self) -> Option<ProbeReport> {
        lock(&self.status).probe.clone()
    }

    /// 解除冲突暂停（模式切到非 `ProxyDhcp` 或冲突被确认）。
    pub fn resume(&self) {
        lock(&self.status).paused = false;
    }

    /// 是否因冲突暂停应答。
    #[must_use]
    pub fn paused(&self) -> bool {
        lock(&self.status).paused
    }

    /// 健康状态与说明。
    #[must_use]
    pub fn health(&self) -> (Health, Option<String>) {
        let status = lock(&self.status);
        let paused = status.paused;
        let conflicts = status
            .probe
            .as_ref()
            .map_or_else(Vec::new, |report| report.conflicts.clone());
        let problems: Vec<String> = status
            .problems
            .iter()
            .map(|(source, detail)| format!("{source}: {detail}"))
            .collect();
        drop(status);
        if paused {
            let servers = conflicts
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ");
            return (
                Health::Conflict,
                Some(format!("发现下发引导参数的 DHCP 服务器：{servers}")),
            );
        }
        if problems.is_empty() {
            (Health::Ok, None)
        } else {
            (Health::Degraded, Some(problems.join("; ")))
        }
    }

    /// 组装心跳。
    #[must_use]
    pub fn heartbeat(&self, at: Timestamp) -> Heartbeat {
        let (health, detail) = self.health();
        let status = lock(&self.status);
        let (rev, mode, listening, probe) = (
            status.rev,
            status.mode.clone(),
            status.listening.clone(),
            status.probe.clone(),
        );
        drop(status);
        Heartbeat {
            at,
            version: env!("CARGO_PKG_VERSION").to_owned(),
            rev,
            mode,
            health,
            detail,
            listening,
            counters: self.counters(),
            probe,
        }
    }
}

#[cfg(test)]
mod tests;
