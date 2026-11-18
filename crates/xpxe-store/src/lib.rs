//! x-pxe 持久化层：SQLite 存储与迁移、不可变配置版本链、审计哈希链、
//! BMC 密码信封加密、账户/会话/API Token 与装机令牌。
//!
//! 对外是同步 API；控制面通过 `tokio::task::spawn_blocking` 调用。
//! 内部为单写连接 + 多读连接（WAL）。领域层不读时钟，本 crate 也一样：
//! 所有与时间有关的操作都由调用方传入 [`Timestamp`]。
#![doc(test(attr(deny(warnings), forbid(unsafe_code))))]

mod assets;
mod audit;
mod auth;
mod config;
mod crypto;
mod migrate;
mod nodes;
mod observations;
mod outbox;
mod sqlval;
mod tokens;

use std::fmt::{Display, Formatter};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard};

use rusqlite::{Connection, OpenFlags, Transaction, TransactionBehavior};
use xpxe_domain::{ConfigError, NodeState, Timestamp};

pub use assets::{AssetMeta, AssetRecord, AssetSource, AssetStage, Assets, NewAsset};
pub use audit::{Audit, AuditRecord, ChainReport};
pub use auth::{
    ARGON2_M_KIB, ARGON2_P, ARGON2_T, Accounts, ApiPrincipal, BACKOFF_CAP_SECS, LoginAttempt,
    NewSession, PASSWORD_MAX, PASSWORD_MIN, Role, SESSION_ABSOLUTE, SESSION_IDLE, Scope,
    SessionUser, TOKEN_MAX_TTL, TOKEN_PREFIX, TokenGrant, User, backoff_delay,
};
pub use config::{Authorship, Configs, ObjectInfo, VersionInfo, VersionRef};
pub use crypto::{MasterKey, Secrets};
pub use nodes::{Commit, EventInsert, EventRecord, EventSummary, NewNode, NodeRow, Nodes};
pub use observations::{Observation, Observations, RETENTION};
pub use outbox::{Outbox, OutboxEntry, OutboxItem};
pub use tokens::{
    AgentToken, InstallSession, IssuedSession, Pinned, Resource, ServedDoc, SessionRequest,
    TokenKind, Tokens,
};

/// 数据库文件名。
pub const DB_FILE: &str = "xpxe.db";

/// 读连接数量。
const READERS: usize = 4;

/// 存储层错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreError {
    /// `SQLite` 错误。
    Sqlite(String),
    /// 文件 I/O 错误。
    Io(String),
    /// 配置文档错误。
    Config(Box<ConfigError>),
    /// 乐观并发冲突：调用方携带的头版本已过期。
    Conflict {
        /// 当前头版本。
        current_head: u64,
    },
    /// 对象不存在。
    NotFound(&'static str),
    /// 节点当前状态与事件的转换前状态不符。
    StateChanged {
        /// 节点当前状态。
        current: NodeState,
    },
    /// 主密钥文件缺失、长度不对或格式错误。
    MasterKey(String),
    /// 主密钥与库里登记的 `key_id` 不符。
    KeyMismatch,
    /// 解密或认证失败（密文被篡改、挪用或用错密钥）。
    Crypto,
    /// 锁中毒。
    Poisoned,
    /// 参数不合法。
    Invalid(String),
    /// 一次性文档已经下发过。
    AlreadyServed,
    /// 登录被退避限制，`retry_after` 秒后重试。
    Throttled {
        /// 剩余等待秒数。
        retry_after: u64,
    },
    /// 用户名或口令错误（不区分用户是否存在）。
    InvalidCredentials,
    /// 库内容与预期不一致。
    Corrupt(String),
}

impl Display for StoreError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sqlite(error) => write!(f, "SQLite 错误：{error}"),
            Self::Io(error) => write!(f, "I/O 错误：{error}"),
            Self::Config(error) => write!(f, "配置错误：{error}"),
            Self::Conflict { current_head } => {
                write!(f, "已被他人修改，当前头版本为 {current_head}")
            }
            Self::NotFound(what) => write!(f, "{what} 不存在"),
            Self::StateChanged { current } => write!(f, "节点状态已变为 {current}"),
            Self::MasterKey(error) => write!(f, "主密钥不可用：{error}"),
            Self::KeyMismatch => f.write_str("主密钥与数据库登记的 key_id 不符"),
            Self::Crypto => f.write_str("解密失败"),
            Self::Poisoned => f.write_str("数据库连接锁已中毒"),
            Self::Invalid(error) => write!(f, "参数不合法：{error}"),
            Self::AlreadyServed => f.write_str("一次性文档已下发"),
            Self::Throttled { retry_after } => write!(f, "请 {retry_after} 秒后再试"),
            Self::InvalidCredentials => f.write_str("用户名或口令错误"),
            Self::Corrupt(error) => write!(f, "数据不一致：{error}"),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<rusqlite::Error> for StoreError {
    fn from(value: rusqlite::Error) -> Self {
        Self::Sqlite(value.to_string())
    }
}

impl From<std::io::Error> for StoreError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value.to_string())
    }
}

impl From<ConfigError> for StoreError {
    fn from(value: ConfigError) -> Self {
        Self::Config(Box::new(value))
    }
}

impl From<serde_json::Error> for StoreError {
    fn from(value: serde_json::Error) -> Self {
        Self::Corrupt(value.to_string())
    }
}

/// 存储层结果。
pub type StoreResult<T> = Result<T, StoreError>;

/// 打开的数据库：单写连接 + 多读连接，以及内存中的主密钥。
#[derive(Debug)]
pub struct Store {
    /// 唯一写连接。
    writer: Mutex<Connection>,
    /// 只读连接池。
    readers: Vec<Mutex<Connection>>,
    /// 下一次优先尝试的读连接。
    next_reader: AtomicUsize,
    /// 主密钥（只在内存）。
    key: MasterKey,
    /// 数据库文件路径。
    path: PathBuf,
}

impl Store {
    /// 打开（必要时创建）`data_dir/xpxe.db`，执行迁移并核对主密钥。
    ///
    /// 首次打开时登记主密钥的 `key_id`；之后每次打开都必须使用同一把主密钥。
    ///
    /// # Errors
    ///
    /// 目录或数据库无法打开、迁移失败、主密钥与登记不符时返回错误。
    pub fn open(data_dir: &Path, master_key: MasterKey) -> StoreResult<Self> {
        std::fs::create_dir_all(data_dir)?;
        let path = data_dir.join(DB_FILE);
        let mut writer = Connection::open(&path)?;
        configure(&writer)?;
        writer.pragma_update(None, "journal_mode", "WAL")?;
        migrate::run(&mut writer)?;
        crypto::check_key_id(&mut writer, &master_key)?;
        let mut readers = Vec::with_capacity(READERS);
        for _ in 0..READERS {
            let reader = Connection::open_with_flags(
                &path,
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            )?;
            configure(&reader)?;
            readers.push(Mutex::new(reader));
        }
        Ok(Self {
            writer: Mutex::new(writer),
            readers,
            next_reader: AtomicUsize::new(0),
            key: master_key,
            path,
        })
    }

    /// 数据库文件路径。
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// 当前 schema 版本（`PRAGMA user_version`）。
    ///
    /// # Errors
    ///
    /// 查询失败时返回错误。
    pub fn schema_version(&self) -> StoreResult<u32> {
        self.read(|conn| {
            let raw = sqlval::query_i64(conn, "PRAGMA user_version", [])?;
            u32::try_from(raw).map_err(|error| StoreError::Corrupt(error.to_string()))
        })
    }

    /// 在线备份到 `dest`（`VACUUM INTO`），目标文件必须不存在。
    ///
    /// # Errors
    ///
    /// 目标已存在或备份失败时返回错误。
    pub fn backup(&self, dest: &Path) -> StoreResult<()> {
        if dest.exists() {
            return Err(StoreError::Invalid(format!(
                "备份目标已存在：{}",
                dest.display()
            )));
        }
        let target = dest
            .to_str()
            .ok_or_else(|| StoreError::Invalid("备份路径不是 UTF-8".to_owned()))?
            .to_owned();
        let conn = self.lock_writer()?;
        sqlval::exec(&conn, "VACUUM INTO ?1", [target])?;
        Ok(())
    }

    /// `PRAGMA integrity_check`，返回 `ok` 以外的全部消息；为空表示完好。
    ///
    /// # Errors
    ///
    /// 查询失败时返回错误。
    pub fn integrity_check(&self) -> StoreResult<Vec<String>> {
        self.read(integrity_messages)
    }

    /// 配置对象与版本链。
    #[must_use]
    pub const fn configs(&self) -> Configs<'_> {
        Configs::new(self)
    }

    /// 节点运行态与事件。
    #[must_use]
    pub const fn nodes(&self) -> Nodes<'_> {
        Nodes::new(self)
    }

    /// 用户、会话、API Token 与登录退避。
    #[must_use]
    pub const fn accounts(&self) -> Accounts<'_> {
        Accounts::new(self)
    }

    /// 装机会话与令牌。
    #[must_use]
    pub const fn tokens(&self) -> Tokens<'_> {
        Tokens::new(self)
    }

    /// 资产元数据。
    #[must_use]
    pub const fn assets(&self) -> Assets<'_> {
        Assets::new(self)
    }

    /// 审计哈希链。
    #[must_use]
    pub const fn audit(&self) -> Audit<'_> {
        Audit::new(self)
    }

    /// BMC 密码密文。
    #[must_use]
    pub const fn secrets(&self) -> Secrets<'_> {
        Secrets::new(self, &self.key)
    }

    /// 副作用发件箱。
    #[must_use]
    pub const fn outbox(&self) -> Outbox<'_> {
        Outbox::new(self)
    }

    /// bootnet 观测。
    #[must_use]
    pub const fn observations(&self) -> Observations<'_> {
        Observations::new(self)
    }

    /// 用旧主密钥打开 `data_dir`，在一个事务内用新主密钥重加密全部 BMC 密文并更新 `key_id`。
    ///
    /// # Errors
    ///
    /// 旧密钥与登记不符、任一密文解密失败或写入失败时返回错误，库保持原样。
    pub fn rotate_master_key(data_dir: &Path, old: MasterKey, new: &MasterKey) -> StoreResult<()> {
        let store = Self::open(data_dir, old)?;
        store.write(|tx| crypto::rotate(tx, &store.key, new))
    }

    /// 在写连接上执行一个 `IMMEDIATE` 事务；闭包返回错误时回滚。
    ///
    /// # Errors
    ///
    /// 闭包或提交失败时返回错误。
    pub(crate) fn write<T, F>(&self, op: F) -> StoreResult<T>
    where
        F: FnOnce(&Transaction<'_>) -> StoreResult<T>,
    {
        let mut conn = self.lock_writer()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let value = op(&tx)?;
        tx.commit()?;
        drop(conn);
        Ok(value)
    }

    /// 在一个读连接上执行查询。
    ///
    /// # Errors
    ///
    /// 闭包失败或锁中毒时返回错误。
    pub(crate) fn read<T, F>(&self, op: F) -> StoreResult<T>
    where
        F: FnOnce(&Connection) -> StoreResult<T>,
    {
        let start = self.next_reader.fetch_add(1, Ordering::Relaxed);
        let count = self.readers.len();
        for step in 0..count {
            let slot = start.wrapping_add(step).checked_rem(count).unwrap_or(0);
            if let Some(reader) = self.readers.get(slot)
                && let Ok(conn) = reader.try_lock()
            {
                return op(&conn);
            }
        }
        let slot = start.checked_rem(count).unwrap_or(0);
        let reader = self
            .readers
            .get(slot)
            .ok_or_else(|| StoreError::Corrupt("没有读连接".to_owned()))?;
        let conn = reader.lock().map_err(|_poisoned| StoreError::Poisoned)?;
        op(&conn)
    }

    /// 取得写连接。
    ///
    /// # Errors
    ///
    /// 锁中毒时返回错误。
    fn lock_writer(&self) -> StoreResult<MutexGuard<'_, Connection>> {
        self.writer.lock().map_err(|_poisoned| StoreError::Poisoned)
    }
}

/// 每个连接的公共设置。
///
/// # Errors
///
/// PRAGMA 失败时返回错误。
fn configure(conn: &Connection) -> StoreResult<()> {
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "synchronous", "FULL")?;
    Ok(())
}

/// `PRAGMA integrity_check` 中 `ok` 以外的行。
///
/// # Errors
///
/// 查询失败时返回错误。
fn integrity_messages(conn: &Connection) -> StoreResult<Vec<String>> {
    let lines = sqlval::query_vec(conn, "PRAGMA integrity_check", [], |row| {
        Ok(row.get::<_, String>(0)?)
    })?;
    Ok(lines.into_iter().filter(|line| line != "ok").collect())
}

/// 当前时间是否晚于某个时间戳。供各模块共享的小工具。
#[must_use]
pub(crate) const fn expired(deadline: Timestamp, now: Timestamp) -> bool {
    now.unix_seconds() >= deadline.unix_seconds()
}
