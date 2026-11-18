//! 共享状态：存储、配置、时钟、推送中心、指令版本与后台唤醒。

use std::collections::BTreeMap;
use std::fmt::{Debug, Formatter};
use std::net::IpAddr;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use serde_json::Value;
use tokio::sync::{Notify, watch};
use xpxe_assets::{AlignmentReport, FetchPolicy, Fetcher, TrustStore};
use xpxe_domain::{HeartbeatRecord, Timestamp};
use xpxe_redfish::Timing;
use xpxe_store::{AuditRecord, MasterKey, Store};

use crate::clock::{Clock, WallTime};
use crate::hub::Hub;
use crate::settings::Settings;
use crate::{Failure, internal};

/// 可注入的运行参数（测试缩短轮询、替换时钟与信任根）。
#[derive(Debug, Clone)]
pub struct Options {
    /// 时钟。
    pub clock: Arc<dyn Clock>,
    /// Debian 信任根；`None` 用内置 Debian 密钥。
    pub trust: Option<TrustStore>,
    /// 定时器与发件箱的最长休眠（规范上限 5 s）。
    pub poll: Duration,
    /// Redfish 超时。
    pub redfish_timing: Timing,
    /// 引导端每 IP 每分钟请求上限。
    pub boot_rate_per_min: u32,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            clock: Arc::new(WallTime),
            trust: None,
            poll: Duration::from_secs(5),
            redfish_timing: Timing::default(),
            boot_rate_per_min: 600,
        }
    }
}

/// 可变的内存状态。
#[derive(Debug, Default)]
pub(crate) struct Volatile {
    /// 最近一次 bootnet 心跳。
    pub(crate) heartbeat: Option<HeartbeatRecord>,
    /// 对齐检查缓存：（资产 ID，镜像 URL）→ 报告。
    pub(crate) alignment: BTreeMap<(i64, String), AlignmentReport>,
    /// 引导端限速：IP →（分钟序号，计数）。
    pub(crate) rate: BTreeMap<IpAddr, (u64, u32)>,
    /// `/boot/mac` 命中：会话 → 首个源 IP。
    pub(crate) boot_ips: BTreeMap<String, IpAddr>,
    /// 被第二个源 IP 命中的会话（节点 ID → 说明）。
    pub(crate) alerts: BTreeMap<i64, String>,
}

/// 控制面共享状态。
pub struct App {
    /// 静态配置。
    pub(crate) settings: Settings,
    /// 存储。
    pub(crate) store: Store,
    /// 时钟。
    pub(crate) clock: Arc<dyn Clock>,
    /// 运行参数。
    pub(crate) options: Options,
    /// `Bearer <bootnet 令牌>`。
    pub(crate) bootnet_auth: String,
    /// 指令版本号。
    pub(crate) rev: watch::Sender<u64>,
    /// 推送中心。
    pub(crate) hub: Hub,
    /// 发件箱唤醒。
    pub(crate) effects_wake: Notify,
    /// 定时器唤醒。
    pub(crate) timers_wake: Notify,
    /// 内存状态。
    volatile: Mutex<Volatile>,
    /// 下载器。
    pub(crate) fetcher: Fetcher,
    /// 信任根。
    pub(crate) trust: TrustStore,
}

impl Debug for App {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("App")
            .field("settings", &self.settings)
            .field("rev", &*self.rev.borrow())
            .finish_non_exhaustive()
    }
}

/// 读取 bootnet 令牌。
///
/// # Errors
///
/// 文件不可读或令牌过短时返回错误。
fn read_bootnet_token(settings: &Settings) -> Result<String, Failure> {
    let raw = std::fs::read_to_string(&settings.bootnet_token_file).map_err(|error| {
        Failure::bad_request(
            "BAD_SETTING",
            format!("读取 {}：{error}", settings.bootnet_token_file.display()),
        )
    })?;
    let token = raw.trim();
    if token.len() < 16 {
        return Err(Failure::bad_request(
            "BAD_SETTING",
            "bootnet 令牌至少 16 个字符",
        ));
    }
    Ok(format!("Bearer {token}"))
}

impl App {
    /// 打开数据目录并建立共享状态（含初始管理员）。
    ///
    /// # Errors
    ///
    /// 主密钥、数据库、令牌文件或初始管理员失败时返回错误。
    pub fn open(settings: Settings, options: Options) -> Result<Arc<Self>, Failure> {
        let (key, warnings) = MasterKey::load(&settings.master_key_file)?;
        for warning in warnings {
            tracing::warn!(%warning, "主密钥文件权限");
        }
        for dir in [
            settings.assets_dir(),
            settings.sessions_dir(),
            settings.iso_dir(),
        ] {
            std::fs::create_dir_all(&dir).map_err(internal)?;
        }
        let store = Store::open(&settings.data_dir, key)?;
        let now = options.clock.now();
        let created = store
            .accounts()
            .ensure_initial_admin(Some(&settings.admin_password_file), now)?;
        if created {
            tracing::info!("已创建初始管理员 admin（首次登录须改密）");
        }
        let bootnet_auth = read_bootnet_token(&settings)?;
        let trust = match &options.trust {
            Some(trust) => trust.clone(),
            None => TrustStore::debian().map_err(internal)?,
        };
        let (rev, _receiver) = watch::channel(now.unix_seconds());
        Ok(Arc::new(Self {
            fetcher: Fetcher::new(FetchPolicy::standard(settings.offline)),
            clock: Arc::clone(&options.clock),
            settings,
            store,
            options,
            bootnet_auth,
            rev,
            hub: Hub::new(),
            effects_wake: Notify::new(),
            timers_wake: Notify::new(),
            volatile: Mutex::new(Volatile::default()),
            trust,
        }))
    }

    /// 当前时间。
    #[must_use]
    pub fn now(&self) -> Timestamp {
        self.clock.now()
    }

    /// 存储。
    #[must_use]
    pub const fn store(&self) -> &Store {
        &self.store
    }

    /// 内存状态锁。
    pub(crate) fn volatile(&self) -> MutexGuard<'_, Volatile> {
        self.volatile.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// 递增指令版本号（bootnet 长轮询立即返回）。
    pub(crate) fn bump_directives(&self) {
        self.rev.send_modify(|rev| *rev = rev.saturating_add(1));
    }

    /// 推送一条 SSE。
    pub(crate) fn push(&self, kind: &str, data: Value) {
        self.hub.publish(kind, data);
    }

    /// 唤醒发件箱与定时器。
    pub(crate) fn wake(&self) {
        self.effects_wake.notify_one();
        self.timers_wake.notify_one();
    }

    /// 写一条审计（失败只记日志，不影响主流程）。
    pub(crate) fn audit(&self, record: &AuditRecord) {
        if let Err(error) = self.store.audit().append(record, self.now()) {
            tracing::error!(%error, action = %record.action, "写审计失败");
        }
    }
}
