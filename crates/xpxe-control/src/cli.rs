//! 命令行：`serve`、`healthcheck`、`audit verify|export --since`、`rotate-master-key`、`backup`、`version`。
//! 运维子命令不依赖监听配置，只读 `XPXE_DATA_DIR` 与 `XPXE_MASTER_KEY_FILE`。

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpStream;
use xpxe_domain::Timestamp;
use xpxe_store::{MasterKey, Store};

use crate::app::{App, Options};
use crate::server;
use crate::settings::Settings;

/// 用法说明。
const USAGE: &str = "用法：xpxe-control <serve | healthcheck | audit verify | audit export --since <RFC3339|unix秒> \
| rotate-master-key --old-file <旧> --new-file <新> | backup --out <文件> | version>";

/// 解析出的命令。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// 启动服务。
    Serve,
    /// 探活（容器 HEALTHCHECK）。
    Healthcheck,
    /// 校验审计哈希链。
    AuditVerify,
    /// 导出审计 JSONL。
    AuditExport {
        /// 起始时间。
        since: Timestamp,
    },
    /// 轮换主密钥。
    Rotate {
        /// 旧密钥文件。
        old: PathBuf,
        /// 新密钥文件。
        new: PathBuf,
    },
    /// 在线备份数据库。
    Backup {
        /// 目标文件。
        out: PathBuf,
    },
    /// 输出版本。
    Version,
}

/// 取 `--flag value`。
fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|arg| arg == name)
        .and_then(|index| args.get(index.saturating_add(1)))
        .cloned()
}

/// 解析时间：RFC 3339 或 Unix 秒。
///
/// # Errors
///
/// 两种格式都不符时返回说明。
pub fn parse_since(raw: &str) -> Result<Timestamp, String> {
    if let Ok(secs) = raw.parse::<u64>() {
        return Ok(Timestamp::from_unix_seconds(secs));
    }
    let parsed = OffsetDateTime::parse(raw, &Rfc3339)
        .map_err(|error| format!("--since 无法解析：{error}"))?;
    u64::try_from(parsed.unix_timestamp())
        .map(Timestamp::from_unix_seconds)
        .map_err(|_negative| "--since 早于 1970".to_owned())
}

/// 解析参数（不含程序名）。
///
/// # Errors
///
/// 未知子命令或缺少参数时返回用法说明。
pub fn parse(args: &[String]) -> Result<Command, String> {
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    let need = |name: &str| flag(args, name).ok_or_else(|| format!("缺少 {name}\n{USAGE}"));
    match words.as_slice() {
        ["serve", ..] => Ok(Command::Serve),
        ["healthcheck", ..] => Ok(Command::Healthcheck),
        ["audit", "verify", ..] => Ok(Command::AuditVerify),
        ["audit", "export", ..] => Ok(Command::AuditExport {
            since: parse_since(&need("--since")?)?,
        }),
        ["rotate-master-key", ..] => Ok(Command::Rotate {
            old: PathBuf::from(need("--old-file")?),
            new: PathBuf::from(need("--new-file")?),
        }),
        ["backup", ..] => Ok(Command::Backup {
            out: PathBuf::from(need("--out")?),
        }),
        ["version" | "--version", ..] => Ok(Command::Version),
        _ => Err(USAGE.to_owned()),
    }
}

/// 环境变量或默认值。
fn env_path(key: &str, default: &str) -> PathBuf {
    PathBuf::from(
        std::env::var(key)
            .ok()
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| default.to_owned()),
    )
}

/// 打开数据库（运维子命令）。
///
/// # Errors
///
/// 主密钥或数据库不可用时返回说明。
fn open_store() -> Result<Store, String> {
    let (key, _warnings) = MasterKey::load(&env_path(
        "XPXE_MASTER_KEY_FILE",
        "/run/secrets/xpxe_master_key",
    ))
    .map_err(|error| error.to_string())?;
    Store::open(&env_path("XPXE_DATA_DIR", "/data"), key).map_err(|error| error.to_string())
}

/// 写一行到标准输出。
///
/// # Errors
///
/// 写入失败时返回说明。
fn say(text: &str) -> Result<(), String> {
    writeln!(std::io::stdout().lock(), "{text}").map_err(|error| error.to_string())
}

/// 探活：GET 本机引导端 `/healthz`。
///
/// # Errors
///
/// 连接失败或非 200 时返回说明。
async fn healthcheck() -> Result<(), String> {
    let listen =
        std::env::var("XPXE_BOOT_LISTEN").unwrap_or_else(|_missing| "0.0.0.0:8080".to_owned());
    let port = listen.rsplit(':').next().unwrap_or("8080");
    let probe = async {
        let mut stream = TcpStream::connect(format!("127.0.0.1:{port}")).await?;
        stream
            .write_all(b"GET /healthz HTTP/1.0\r\nHost: localhost\r\n\r\n")
            .await?;
        let mut head = vec![0_u8; 64];
        let read = stream.read(&mut head).await?;
        Ok::<_, std::io::Error>(head.get(..read).is_some_and(|bytes| {
            bytes.starts_with(b"HTTP/1.1 200") || bytes.starts_with(b"HTTP/1.0 200")
        }))
    };
    match tokio::time::timeout(Duration::from_secs(5), probe).await {
        Ok(Ok(true)) => Ok(()),
        Ok(Ok(false)) => Err("healthz 非 200".to_owned()),
        Ok(Err(error)) => Err(format!("healthz 连接失败：{error}")),
        Err(_elapsed) => Err("healthz 超时".to_owned()),
    }
}

/// 等待 SIGTERM 或 Ctrl-C。
async fn terminated() {
    let term = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                let _received = signal.recv().await;
            }
            Err(error) => {
                tracing::warn!(%error, "无法监听 SIGTERM");
                std::future::pending::<()>().await;
            }
        }
    };
    tokio::select! {
        () = term => {}
        _ctrl = tokio::signal::ctrl_c() => {}
    }
}

/// 启动服务直到收到停止信号。
///
/// # Errors
///
/// 配置、数据目录、证书或端口不可用时返回说明。
async fn serve() -> Result<(), String> {
    let settings = Settings::from_env().map_err(|error| error.to_string())?;
    let app: Arc<App> =
        App::open(settings, Options::default()).map_err(|error| error.to_string())?;
    let running = server::start(app)
        .await
        .map_err(|error| error.to_string())?;
    tracing::info!(admin = %running.admin_addr(), boot = %running.boot_addr(), internal = %running.internal_addr(), "xpxe-control 已启动");
    terminated().await;
    tracing::info!("收到停止信号，优雅退出");
    running.shutdown().await;
    Ok(())
}

/// 轮换主密钥。
///
/// # Errors
///
/// 密钥文件或重加密失败时返回说明。
fn rotate(old: &Path, new: &Path) -> Result<(), String> {
    let (old_key, _old_warnings) = MasterKey::load(old).map_err(|error| error.to_string())?;
    let (new_key, _new_warnings) = MasterKey::load(new).map_err(|error| error.to_string())?;
    Store::rotate_master_key(&env_path("XPXE_DATA_DIR", "/data"), old_key, &new_key)
        .map_err(|error| error.to_string())?;
    say("主密钥已轮换；请把 XPXE_MASTER_KEY_FILE 指向新密钥后重启")
}

/// 执行命令。
///
/// # Errors
///
/// 命令失败时返回说明（调用方打印并以非零码退出）。
pub async fn run(command: Command) -> Result<(), String> {
    match command {
        Command::Serve => serve().await,
        Command::Healthcheck => healthcheck().await,
        Command::AuditVerify => {
            let report = open_store()?
                .audit()
                .verify()
                .map_err(|error| error.to_string())?;
            report.first_broken.map_or_else(
                || say(&format!("审计链完整：{} 条", report.checked)),
                |id| {
                    Err(format!(
                        "审计链在第 {id} 条断开（已检查 {} 条）",
                        report.checked
                    ))
                },
            )
        }
        Command::AuditExport { since } => {
            let mut out = std::io::stdout().lock();
            let _rows = open_store()?
                .audit()
                .export_since(since, &mut out)
                .map_err(|error| error.to_string())?;
            out.flush().map_err(|error| error.to_string())
        }
        Command::Rotate { old, new } => rotate(&old, &new),
        Command::Backup { out } => {
            open_store()?
                .backup(&out)
                .map_err(|error| error.to_string())?;
            say(&format!("已备份到 {}", out.display()))
        }
        Command::Version => say(&format!(
            "{} {}",
            env!("CARGO_PKG_NAME"),
            env!("CARGO_PKG_VERSION")
        )),
    }
}

#[cfg(test)]
mod tests;
