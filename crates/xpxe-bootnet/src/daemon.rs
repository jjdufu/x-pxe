//! 常驻进程装配：指令同步、失联看门狗、观测上报、心跳、现网探测与监听管理；以及 `healthcheck`。

use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use tokio::sync::watch;
use xpxe_dhcp::link::UdpTransport;
use xpxe_domain::{BootNetwork, DirectiveSnapshot, HttpUrl, NetIface, Observation, ProbeReport};

use crate::failsafe::{Liveness, effective};
use crate::listeners::{Ports, Supervisor};
use crate::memory::FirmwareCache;
use crate::netinfo::{iface_ipv4, socket_error};
use crate::scan::{COLLECT_FOR, probe, seed};
use crate::settings::Settings;
use crate::state::Shared;
use crate::upstream::ControlClient;
use crate::{BootnetError, wall_clock};

/// 观测上报周期。
const UPLOAD_EVERY: Duration = Duration::from_secs(2);
/// 心跳周期。
const HEARTBEAT_EVERY: Duration = Duration::from_secs(10);
/// 现网探测周期。
const PROBE_EVERY: Duration = Duration::from_secs(600);
/// 拉取失败后的重试间隔。
const RETRY_AFTER: Duration = Duration::from_secs(2);
/// 健康检查允许的心跳最大年龄（秒）。
pub const HEALTHY_WITHIN_SECS: u64 = 30;

/// 生效快照接收端。
pub type SnapshotReceiver = watch::Receiver<Arc<DirectiveSnapshot>>;

/// 指令发布器：最新快照 + 失联状态 → 生效快照。
#[derive(Debug)]
pub struct Publisher {
    /// 生效快照发送端。
    sender: watch::Sender<Arc<DirectiveSnapshot>>,
    /// 内部状态。
    inner: Mutex<PublisherState>,
    /// 允许的网卡。
    allowed: Vec<NetIface>,
    /// 共享状态。
    shared: Arc<Shared>,
}

/// 发布器内部状态。
#[derive(Debug)]
struct PublisherState {
    /// 最近一次从控制面收到的快照。
    latest: Option<Arc<DirectiveSnapshot>>,
    /// 最近一次成功通信。
    liveness: Liveness,
    /// 当前是否以失联状态发布。
    stale: bool,
}

/// 启动时（尚未联系到控制面）的生效快照：`RedfishOnly`、无指令。
///
/// # Errors
///
/// 不会失败；URL 解析失败时返回错误以满足类型要求。
pub fn closed_snapshot() -> Result<DirectiveSnapshot, BootnetError> {
    let base = HttpUrl::parse("http://127.0.0.1:8080")
        .map_err(|error| BootnetError::new("internal", error.code()))?;
    Ok(DirectiveSnapshot::closed(0, BootNetwork::RedfishOnly, base))
}

impl Publisher {
    /// 新建发布器与生效快照接收端。
    ///
    /// # Errors
    ///
    /// 见 [`closed_snapshot`]。
    pub fn new(
        allowed: Vec<NetIface>,
        shared: Arc<Shared>,
        now: Instant,
    ) -> Result<(Arc<Self>, SnapshotReceiver), BootnetError> {
        let (sender, receiver) = watch::channel(Arc::new(closed_snapshot()?));
        let publisher = Arc::new(Self {
            sender,
            inner: Mutex::new(PublisherState {
                latest: None,
                liveness: Liveness::new(now),
                stale: false,
            }),
            allowed,
            shared,
        });
        Ok((publisher, receiver))
    }

    /// 当前已应用的版本。
    #[must_use]
    pub fn applied_rev(&self) -> Option<u64> {
        self.lock().latest.as_ref().map(|snapshot| snapshot.rev)
    }

    /// 取内部锁。
    fn lock(&self) -> std::sync::MutexGuard<'_, PublisherState> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// 收到新快照（已确保固件就绪）。
    pub fn accept(&self, snapshot: DirectiveSnapshot, now: Instant) {
        let latest = Arc::new(snapshot);
        let mut state = self.lock();
        state.liveness.mark_ok(now);
        state.stale = false;
        state.latest = Some(Arc::clone(&latest));
        drop(state);
        self.publish(&latest, false);
    }

    /// 304：通信正常、无变化。
    pub fn keepalive(&self, now: Instant) {
        let mut state = self.lock();
        state.liveness.mark_ok(now);
        let recovered = state.stale;
        state.stale = false;
        let latest = state.latest.clone();
        drop(state);
        if recovered && let Some(snapshot) = latest {
            self.publish(&snapshot, false);
        }
    }

    /// 看门狗：失联超过 30 s 时清空指令表并降级。
    pub fn tick(&self, now: Instant) {
        let mut state = self.lock();
        let expired = state.liveness.is_stale(now) && !state.stale;
        if expired {
            state.stale = true;
        }
        let latest = state.latest.clone();
        drop(state);
        if expired {
            self.shared.set_problem(
                "control",
                "超过 30 s 未能从控制面拉取指令，已清空应答指令表".to_owned(),
            );
            if let Some(snapshot) = latest {
                self.publish(&snapshot, true);
            }
        }
    }

    /// 发布生效快照。
    fn publish(&self, latest: &DirectiveSnapshot, stale: bool) {
        if !stale {
            self.shared.clear_problem("control");
        }
        let mut next = effective(latest, stale);
        if let Err(errors) = latest.network.check_allowed(&self.allowed) {
            let detail = errors
                .first()
                .map_or_else(String::new, |error| error.message.clone());
            self.shared.set_problem("iface", detail);
            next.network = BootNetwork::RedfishOnly;
            next.directives.clear();
        } else {
            self.shared.clear_problem("iface");
        }
        self.shared.set_applied(next.rev, next.network.label());
        let _previous: Arc<DirectiveSnapshot> = self.sender.send_replace(Arc::new(next));
    }
}

/// 同步循环：长轮询指令、下载缺失固件、发布。
async fn sync_loop(
    client: ControlClient,
    publisher: Arc<Publisher>,
    cache: Arc<FirmwareCache>,
    shared: Arc<Shared>,
) {
    while shared.running() {
        match client.directives(publisher.applied_rev()).await {
            Ok(Some(snapshot)) => {
                let complete = fetch_firmware(&client, &cache, &shared, &snapshot).await;
                cache.retain(&snapshot.referenced_firmware());
                let rev = snapshot.rev;
                publisher.accept(snapshot, Instant::now());
                if complete {
                    shared.clear_problem("firmware");
                } else {
                    tracing::warn!(rev, "部分固件未就绪，稍后重试");
                    tokio::time::sleep(RETRY_AFTER).await;
                }
            }
            Ok(None) => publisher.keepalive(Instant::now()),
            Err(error) => {
                tracing::warn!(%error, "拉取指令失败");
                tokio::time::sleep(RETRY_AFTER).await;
            }
        }
    }
}

/// 下载快照引用但尚未缓存的固件；返回是否全部就绪。
async fn fetch_firmware(
    client: &ControlClient,
    cache: &FirmwareCache,
    shared: &Shared,
    snapshot: &DirectiveSnapshot,
) -> bool {
    let mut complete = true;
    for digest in cache.missing(&snapshot.referenced_firmware()) {
        match client.firmware(&digest).await {
            Ok(bytes) => cache.insert(digest, bytes),
            Err(error) => {
                complete = false;
                shared.set_problem("firmware", error.to_string());
            }
        }
    }
    complete
}

/// 看门狗：每秒检查一次失联。
async fn watchdog(publisher: Arc<Publisher>, shared: Arc<Shared>) {
    let mut ticker = tokio::time::interval(Duration::from_secs(1));
    while shared.running() {
        let _instant = ticker.tick().await;
        publisher.tick(Instant::now());
    }
}

/// 观测上报：每 2 s 或满 200 条。
async fn uploader(client: ControlClient, shared: Arc<Shared>) {
    while shared.running() {
        let _woken: Result<(), tokio::time::error::Elapsed> =
            tokio::time::timeout(UPLOAD_EVERY, shared.batch_ready()).await;
        flush(&client, &shared).await;
    }
}

/// 上报队列中的全部观测；失败时放回队列等下次。
async fn flush(client: &ControlClient, shared: &Shared) {
    let mut batch = shared.take_batch();
    while !batch.is_empty() {
        if let Err(error) = client.post_observations(&batch).await {
            tracing::warn!(%error, "上报观测失败");
            shared.requeue(batch);
            return;
        }
        batch = shared.take_batch();
    }
}

/// 心跳：每 10 s。
async fn heartbeat(client: ControlClient, shared: Arc<Shared>) {
    let mut ticker = tokio::time::interval(HEARTBEAT_EVERY);
    while shared.running() {
        let _instant = ticker.tick().await;
        if let Err(error) = client.post_heartbeat(&shared.heartbeat(wall_clock())).await {
            tracing::warn!(%error, "心跳失败");
        }
    }
}

/// 现网探测：启动时、模式切换时、此后每 10 分钟；快照中的确认列表变化时重新评估。
async fn prober(
    shared: Arc<Shared>,
    mut snapshot: watch::Receiver<Arc<DirectiveSnapshot>>,
    probe_port: u16,
) {
    let mut last_network: Option<BootNetwork> = None;
    let mut last_run: Option<Instant> = None;
    loop {
        let current = Arc::clone(&snapshot.borrow_and_update());
        let due = last_run.is_none_or(|at| at.elapsed() >= PROBE_EVERY);
        if last_network.as_ref() != Some(&current.network) || due {
            last_network = Some(current.network.clone());
            last_run = Some(Instant::now());
            run_probe(&shared, &current, probe_port).await;
        } else {
            reassess(&shared, &current);
        }
        let wait = last_run.map_or(PROBE_EVERY, |at| PROBE_EVERY.saturating_sub(at.elapsed()));
        if let Ok(Err(_closed)) = tokio::time::timeout(wait, snapshot.changed()).await {
            return;
        }
    }
}

/// 执行一次探测并更新冲突状态。
async fn run_probe(shared: &Shared, snapshot: &DirectiveSnapshot, probe_port: u16) {
    let Some(iface) = snapshot.network.iface() else {
        shared.resume();
        shared.clear_problem("probe");
        return;
    };
    let local = match iface_ipv4(iface.as_str()) {
        Ok(address) => address,
        Err(error) => {
            shared.set_problem("probe", error.to_string());
            return;
        }
    };
    let transport = match UdpTransport::bind(iface.as_str(), probe_port) {
        Ok(transport) => transport,
        Err(error) => {
            shared.set_problem(
                "probe",
                socket_error(iface.as_str(), "绑定探测端口 ", &error).to_string(),
            );
            return;
        }
    };
    match probe(&transport, seed(), COLLECT_FOR).await {
        Ok(responders) => {
            shared.clear_problem("probe");
            let report = ProbeReport::assess(
                wall_clock(),
                responders,
                &snapshot.network,
                local,
                &snapshot.acknowledged_conflicts,
            );
            record_probe(shared, report);
        }
        Err(error) => shared.set_problem("probe", error.to_string()),
    }
}

/// 用新的确认列表重新评估最近一次探测结果。
fn reassess(shared: &Shared, snapshot: &DirectiveSnapshot) {
    let Some(previous) = shared.probe() else {
        return;
    };
    let local = snapshot
        .network
        .iface()
        .and_then(|iface| iface_ipv4(iface.as_str()).ok());
    let Some(address) = local else {
        return;
    };
    let report = ProbeReport::assess(
        previous.at,
        previous.responders,
        &snapshot.network,
        address,
        &snapshot.acknowledged_conflicts,
    );
    if Some(&report.conflicts) != shared.probe().as_ref().map(|old| &old.conflicts) {
        record_probe(shared, report);
    }
}

/// 记录探测结论与冲突暂停。
fn record_probe(shared: &Shared, report: ProbeReport) {
    let pause = !report.conflicts.is_empty();
    if pause {
        tracing::warn!(conflicts = ?report.conflicts, "发现冲突的 DHCP 引导服务器，暂停应答");
    }
    shared.observe(Observation::Probe {
        at: report.at,
        report: report.clone(),
    });
    let _paused = shared.set_probe(report, pause);
}

/// 运行参数（测试可改端口）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunOptions {
    /// 监听端口。
    pub ports: Ports,
    /// 探测用本地端口（68）。
    pub probe_port: u16,
}

impl Default for RunOptions {
    fn default() -> Self {
        Self {
            ports: Ports::STANDARD,
            probe_port: 68,
        }
    }
}

/// 常驻运行，直到监听管理器结束（正常情况下不返回）。
///
/// # Errors
///
/// 初始化失败时返回错误。
pub async fn run(settings: &Settings, options: RunOptions) -> Result<(), BootnetError> {
    let client = ControlClient::new(settings);
    let shared = Arc::new(Shared::default());
    let cache = Arc::new(FirmwareCache::default());
    let (publisher, receiver) = Publisher::new(
        settings.allowed_ifaces.clone(),
        Arc::clone(&shared),
        Instant::now(),
    )?;
    let background = [
        tokio::spawn(sync_loop(
            client.clone(),
            Arc::clone(&publisher),
            Arc::clone(&cache),
            Arc::clone(&shared),
        )),
        tokio::spawn(watchdog(Arc::clone(&publisher), Arc::clone(&shared))),
        tokio::spawn(uploader(client.clone(), Arc::clone(&shared))),
        tokio::spawn(heartbeat(client, Arc::clone(&shared))),
        tokio::spawn(prober(
            Arc::clone(&shared),
            receiver.clone(),
            options.probe_port,
        )),
    ];
    Supervisor::new(shared, cache, options.ports)
        .run(receiver)
        .await;
    for task in background {
        task.abort();
    }
    Ok(())
}

/// 容器健康检查：最近一次心跳在 30 s 内且自报 `ok`。
///
/// # Errors
///
/// 控制面不可达或响应非法时返回错误。
pub async fn healthcheck(settings: &Settings) -> Result<bool, BootnetError> {
    let record = ControlClient::new(settings).heartbeat().await?;
    Ok(record.is_healthy(wall_clock(), HEALTHY_WITHIN_SECS))
}

#[cfg(test)]
mod tests;
