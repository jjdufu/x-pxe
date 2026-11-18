//! 按模式开关监听：`ProxyDhcp` = 67 + 4011 + 69；`ExternalDhcp` = 69；`RedfishOnly` = 无。
//!
//! 模式切换时先关闭（中止并等待）旧任务以释放套接字，再打开新套接字，并写观测事件。

use std::io;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Instant;

use tokio::sync::watch;
use tokio::task::JoinHandle;
use xpxe_dhcp::link::{DhcpTransport, UdpTransport, bind_udp};
use xpxe_domain::{BootNetwork, DirectiveSnapshot, Observation, TransferStage};
use xpxe_tftp::server::{Observer, Outcome, ServeConfig, serve};
use xpxe_tftp::session::Limits;

use crate::answering::{Context, Engine, Inbound, Verdict};
use crate::memory::{FirmwareCache, TftpFiles};
use crate::netinfo::{iface_ipv4, iface_mtu, socket_error};
use crate::state::{Counter, Shared};
use crate::wall_clock;

/// 监听端口。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ports {
    /// DHCP（67）。
    pub dhcp: u16,
    /// PXE 引导服务（4011）。
    pub pxe: u16,
    /// TFTP（69）。
    pub tftp: u16,
}

impl Ports {
    /// 标准端口。
    pub const STANDARD: Self = Self {
        dhcp: 67,
        pxe: 4011,
        tftp: 69,
    };
}

/// DHCP 收发循环的依赖。
#[derive(Debug, Clone)]
pub struct DhcpLoop {
    /// 应答引擎（两个端口共享限速）。
    pub engine: Arc<Mutex<Engine>>,
    /// 共享状态。
    pub shared: Arc<Shared>,
    /// 当前生效快照。
    pub snapshot: watch::Receiver<Arc<DirectiveSnapshot>>,
    /// 本机地址。
    pub server: Ipv4Addr,
}

impl DhcpLoop {
    /// 收包 → 判定 → 应答 → 观测，直到传输出错。
    ///
    /// # Errors
    ///
    /// 收发失败时返回 I/O 错误。
    pub async fn run<T: DhcpTransport>(self, transport: T) -> io::Result<()> {
        loop {
            let datagram = transport.recv().await?;
            self.shared.count(Counter::DhcpReceived);
            let snapshot = Arc::clone(&self.snapshot.borrow());
            let ctx = Context {
                snapshot: &snapshot,
                server: self.server,
                paused: self.shared.paused(),
                now: Instant::now(),
                wall: wall_clock(),
            };
            let inbound = Inbound {
                payload: &datagram.payload,
                port: transport.port(),
                from: datagram.from,
            };
            let verdict = self
                .engine
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .handle(inbound, &ctx);
            match verdict {
                Verdict::Dropped(counter) => self.shared.count(counter),
                Verdict::Unrelated => self.shared.count(Counter::DhcpIgnored),
                Verdict::Ignored(observation) => {
                    self.shared.count(Counter::DhcpIgnored);
                    self.shared.observe(*observation);
                }
                Verdict::Replied {
                    bytes,
                    to,
                    observation,
                } => {
                    transport.send(&bytes, to).await?;
                    self.shared.count(Counter::DhcpReplies);
                    self.shared.observe(*observation);
                }
            }
        }
    }
}

/// TFTP 观测：写入共享状态。
#[derive(Debug)]
pub struct TftpReporter {
    /// 共享状态。
    shared: Arc<Shared>,
}

impl TftpReporter {
    /// 新建。
    #[must_use]
    pub const fn new(shared: Arc<Shared>) -> Self {
        Self { shared }
    }
}

impl Observer for TftpReporter {
    fn started(&self, peer: std::net::SocketAddr, path: &str) {
        self.shared.observe(Observation::Tftp {
            at: wall_clock(),
            client: peer.ip(),
            file: path.to_owned(),
            stage: TransferStage::Started,
            bytes: 0,
            millis: 0,
        });
    }

    fn finished(&self, outcome: &Outcome) {
        let stage = if outcome.completed {
            self.shared.count(Counter::TftpCompleted);
            TransferStage::Completed
        } else {
            self.shared.count(Counter::TftpFailed);
            TransferStage::Failed
        };
        self.shared.observe(Observation::Tftp {
            at: wall_clock(),
            client: outcome.peer.ip(),
            file: outcome.path.clone(),
            stage,
            bytes: outcome.bytes,
            millis: u64::try_from(outcome.elapsed.as_millis()).unwrap_or(u64::MAX),
        });
    }
}

/// 当前打开的监听。
#[derive(Debug)]
struct Active {
    /// 模式。
    network: BootNetwork,
    /// 任务。
    tasks: Vec<JoinHandle<()>>,
}

/// 监听管理器。
#[derive(Debug)]
pub struct Supervisor {
    /// 共享状态。
    shared: Arc<Shared>,
    /// 固件缓存。
    cache: Arc<FirmwareCache>,
    /// 应答引擎。
    engine: Arc<Mutex<Engine>>,
    /// 端口。
    ports: Ports,
    /// 当前监听。
    active: Option<Active>,
}

impl Supervisor {
    /// 新建（尚未打开任何监听）。
    #[must_use]
    pub fn new(shared: Arc<Shared>, cache: Arc<FirmwareCache>, ports: Ports) -> Self {
        Self {
            shared,
            cache,
            engine: Arc::new(Mutex::new(Engine::new(Instant::now()))),
            ports,
            active: None,
        }
    }

    /// 跟随快照中的模式开关监听，直到快照发送端关闭。
    pub async fn run(mut self, mut snapshot: watch::Receiver<Arc<DirectiveSnapshot>>) {
        loop {
            let network = snapshot.borrow_and_update().network.clone();
            let unchanged = self
                .active
                .as_ref()
                .is_some_and(|active| active.network == network);
            if !unchanged {
                self.switch(&network, &snapshot).await;
            }
            if snapshot.changed().await.is_err() {
                self.stop().await;
                return;
            }
        }
    }

    /// 关闭全部监听（中止任务并等待其释放套接字）。
    pub async fn stop(&mut self) {
        if let Some(active) = self.active.take() {
            for task in active.tasks {
                reap(task).await;
            }
        }
        self.shared.set_listening(Vec::new());
    }

    /// 切换到新模式。
    async fn switch(
        &mut self,
        network: &BootNetwork,
        snapshot: &watch::Receiver<Arc<DirectiveSnapshot>>,
    ) {
        let from = self
            .active
            .as_ref()
            .map_or("none", |active| active.network.label());
        self.shared.observe(Observation::ModeChanged {
            at: wall_clock(),
            from: from.to_owned(),
            to: network.label().to_owned(),
        });
        tracing::info!(from, to = network.label(), "引导网络模式切换");
        self.stop().await;
        self.shared.clear_problem("bind");
        let tasks = network.iface().map_or_else(Vec::new, |iface| {
            self.open(network, iface.as_str(), snapshot)
        });
        self.active = Some(Active {
            network: network.clone(),
            tasks,
        });
    }

    /// 绑定一个 DHCP 端口并启动收发循环；绑定失败时记录问题（不重试抢占）。
    fn spawn_dhcp(&self, iface: &str, port: u16, job: DhcpLoop) -> Option<JoinHandle<()>> {
        match UdpTransport::bind(iface, port) {
            Ok(transport) => Some(tokio::spawn(log_exit("dhcp", port, job.run(transport)))),
            Err(error) => {
                let detail = socket_error(iface, &format!("绑定 {port}/udp "), &error);
                self.shared.set_problem("bind", detail.to_string());
                None
            }
        }
    }

    /// 打开该模式需要的监听；单个端口失败只记录问题，不重试抢占。
    fn open(
        &self,
        network: &BootNetwork,
        iface: &str,
        snapshot: &watch::Receiver<Arc<DirectiveSnapshot>>,
    ) -> Vec<JoinHandle<()>> {
        let server = match iface_ipv4(iface) {
            Ok(address) => address,
            Err(error) => {
                self.shared.set_problem("bind", error.to_string());
                self.shared.set_listening(Vec::new());
                return Vec::new();
            }
        };
        let mut tasks = Vec::new();
        let mut listening = Vec::new();
        let dhcp_ports = if network.serves_dhcp() {
            vec![self.ports.dhcp, self.ports.pxe]
        } else {
            Vec::new()
        };
        for port in dhcp_ports {
            let job = DhcpLoop {
                engine: Arc::clone(&self.engine),
                shared: Arc::clone(&self.shared),
                snapshot: snapshot.clone(),
                server,
            };
            if let Some(task) = self.spawn_dhcp(iface, port, job) {
                tasks.push(task);
                listening.push(port);
            }
        }
        match bind_udp(iface, Ipv4Addr::UNSPECIFIED, self.ports.tftp) {
            Ok(socket) => {
                let config = ServeConfig {
                    limits: Limits::for_mtu(iface_mtu(iface)),
                    ..ServeConfig::new(IpAddr::V4(server))
                };
                let files = Arc::new(TftpFiles::new(Arc::clone(&self.cache), snapshot.clone()));
                let reporter = Arc::new(TftpReporter::new(Arc::clone(&self.shared)));
                tasks.push(tokio::spawn(log_exit(
                    "tftp",
                    self.ports.tftp,
                    serve(socket, files, config, reporter),
                )));
                listening.push(self.ports.tftp);
            }
            Err(error) => self.shared.set_problem(
                "bind",
                socket_error(iface, &format!("绑定 {}/udp ", self.ports.tftp), &error).to_string(),
            ),
        }
        self.shared.set_listening(listening);
        tasks
    }
}

/// 中止任务并等待其结束（释放套接字）。
async fn reap(task: JoinHandle<()>) {
    task.abort();
    if let Err(error) = task.await
        && !error.is_cancelled()
    {
        tracing::warn!(%error, "监听任务异常结束");
    }
}

/// 运行一个监听任务，异常结束时记日志。
async fn log_exit<F: Future<Output = io::Result<()>> + Send>(
    name: &'static str,
    port: u16,
    task: F,
) {
    if let Err(error) = task.await {
        tracing::error!(name, port, %error, "监听任务退出");
    }
}

#[cfg(test)]
mod tests;
