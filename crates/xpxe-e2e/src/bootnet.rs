//! bootnet 模拟：用真实的 `ControlClient`、`Publisher` 与应答引擎消费控制面内部 API，
//! 回放 DHCP 夹具报文，按 TFTP 路径解析并下载固件。

use std::net::{Ipv4Addr, SocketAddrV4};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::Outcome;
use xpxe_bootnet::answering::{Context, Engine, Inbound, Verdict};
use xpxe_bootnet::daemon::{Publisher, SnapshotReceiver};
use xpxe_bootnet::memory::resolve;
use xpxe_bootnet::settings::Settings;
use xpxe_bootnet::state::Shared;
use xpxe_bootnet::upstream::ControlClient;
use xpxe_dhcp::wire::Message;
use xpxe_domain::{DirectiveSnapshot, MacAddr, Timestamp};
use xpxe_test_support::{TestFailure, fixture_bytes};

/// BOOTP 报文中 `chaddr` 的偏移。
const CHADDR: usize = 28;

/// 应答中可比较的字段。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answer {
    /// 启动文件名或 URL。
    pub file: String,
    /// 选项 60。
    pub vendor_class: String,
}

/// 模拟的 bootnet。
#[derive(Debug)]
pub struct Sim {
    /// 内部 API 客户端。
    client: ControlClient,
    /// 快照发布器。
    publisher: Arc<Publisher>,
    /// 生效快照。
    receiver: SnapshotReceiver,
    /// 应答引擎。
    engine: Engine,
    /// 时间原点。
    origin: Instant,
}

/// DHCP 夹具字节。
///
/// # Errors
///
/// 文件缺失时返回错误。
pub fn dhcp_fixture(name: &str) -> Outcome<Vec<u8>> {
    Ok(fixture_bytes(
        concat!(env!("CARGO_MANIFEST_DIR"), "/../xpxe-dhcp"),
        &format!("{name}.bin"),
    )?)
}

impl Sim {
    /// 连接控制面内部 API。
    ///
    /// # Errors
    ///
    /// 配置不合法时返回错误。
    pub fn new(internal: std::net::SocketAddr, token_file: &Path) -> Outcome<Self> {
        let url = format!("http://{internal}");
        let token = token_file.display().to_string();
        let settings = Settings::from_lookup(|key| match key {
            "XPXE_CONTROL_URL" => Some(url.clone()),
            "XPXE_BOOTNET_TOKEN_FILE" => Some(token.clone()),
            "XPXE_BOOTNET_ALLOWED_IFACES" => Some("br-lab".to_owned()),
            _ => None,
        })?;
        let origin = Instant::now();
        let (publisher, receiver) = Publisher::new(
            settings.allowed_ifaces.clone(),
            Arc::new(Shared::default()),
            origin,
        )?;
        Ok(Self {
            client: ControlClient::new(&settings),
            publisher,
            receiver,
            engine: Engine::new(origin),
            origin,
        })
    }

    /// 拉取最新指令并发布。
    ///
    /// # Errors
    ///
    /// 内部 API 失败时返回错误。
    pub async fn sync(&self) -> Outcome<()> {
        let snapshot = self
            .client
            .directives(None)
            .await?
            .ok_or_else(|| TestFailure::new("首次拉取不应返回 304"))?;
        self.publisher.accept(snapshot, Instant::now());
        Ok(())
    }

    /// 当前生效快照。
    #[must_use]
    pub fn current(&self) -> Arc<DirectiveSnapshot> {
        Arc::clone(&self.receiver.borrow())
    }

    /// 模拟时间流逝后检查控制面失联。
    pub fn tick_after(&self, after: Duration) {
        self.publisher
            .tick(self.origin.checked_add(after).unwrap_or(self.origin));
    }

    /// 回放一个 DISCOVER 夹具。
    ///
    /// # Errors
    ///
    /// 夹具缺失或应答无法解析时返回错误。
    pub fn replay(&mut self, fixture: &str, wall: Timestamp) -> Outcome<Option<Answer>> {
        self.replay_as(fixture, None, wall)
    }

    /// 回放夹具，可把 `chaddr` 改写为另一 MAC（同一机器两阶段夹具 MAC 不同时使用）。
    ///
    /// # Errors
    ///
    /// 夹具缺失或应答无法解析时返回错误。
    pub fn replay_as(&mut self, fixture: &str, mac: Option<MacAddr>, wall: Timestamp) -> Outcome<Option<Answer>> {
        let mut payload = dhcp_fixture(fixture)?;
        if let Some(address) = mac
            && let Some(chaddr) = payload.get_mut(CHADDR..CHADDR.saturating_add(6))
        {
            chaddr.copy_from_slice(&address.octets());
        }
        let snapshot = self.current();
        let ctx = Context {
            snapshot: &snapshot,
            server: Ipv4Addr::new(10, 77, 0, 1),
            paused: false,
            now: Instant::now(),
            wall,
        };
        let inbound = Inbound {
            payload: &payload,
            port: 67,
            from: SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 68),
        };
        match self.engine.handle(inbound, &ctx) {
            Verdict::Replied { bytes, .. } => {
                let reply = Message::decode(&bytes)?;
                let vendor_class = reply
                    .option(xpxe_dhcp::wire::code::VENDOR_CLASS)
                    .map_or_else(String::new, |raw| String::from_utf8_lossy(raw).into_owned());
                let file = String::from_utf8_lossy(&reply.file)
                    .trim_end_matches('\0')
                    .to_owned();
                Ok(Some(Answer { file, vendor_class }))
            }
            Verdict::Ignored(_) | Verdict::Unrelated | Verdict::Dropped(_) => Ok(None),
        }
    }

    /// 按 TFTP 路径解析摘要并经内部 API 下载固件（bootnet 的 TFTP 即从此缓存回答）。
    ///
    /// # Errors
    ///
    /// 路径不在快照中或下载失败时返回错误。
    pub async fn tftp(&self, path: &str) -> Outcome<Arc<[u8]>> {
        let digest = resolve(&self.current(), path)
            .ok_or_else(|| TestFailure::new(format!("TFTP 路径 {path} 未解析")))?;
        Ok(self.client.firmware(&digest).await?)
    }
}
