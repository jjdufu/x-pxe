//! `WebUI` 联调栈：真实时钟的进程内控制面 + 模拟 Debian 镜像 + 周期心跳 + 已暂存的 netboot 夹具。
//!
//! 供 `web/e2e/integration.spec.ts` 使用：管理端在固定端口托管 `web/dist`，引导端在固定端口供
//! `xpxe-lab simulate-install` 走完整装机流程。iPXE 资产由页面上传，netboot 夹具已登记待页面激活
//! （netboot 归档无法离线上传，见第 12 步偏差）。

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use xpxe_control::app::{App, Options};
use xpxe_control::server::{self, Running};
use xpxe_control::settings::Settings;
use xpxe_domain::{Health, Heartbeat};
use xpxe_test_support::TestFailure;

use crate::harness::{BOOTNET_TOKEN, INITIAL_PASSWORD, write_secret};
use crate::http::{Call, send};
use crate::{Outcome, mirror, seed};

/// 心跳间隔（控制面判定健康的窗口是 30 秒）。
const BEAT_EVERY: Duration = Duration::from_secs(5);
/// 白名单网卡。
pub const ALLOWED_IFACE: &str = "br-lab";

/// 栈参数。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// 工作目录（每次启动清空其中的 `data/`）。
    pub dir: PathBuf,
    /// 管理端监听地址。
    pub admin: SocketAddr,
    /// 引导端监听地址。
    pub boot: SocketAddr,
    /// 前端构建产物目录。
    pub web: PathBuf,
}

/// 运行中的联调栈。
#[derive(Debug)]
pub struct Stack {
    /// 控制面。
    running: Running,
    /// 模拟镜像地址。
    mirror: SocketAddr,
    /// 暂存的 netboot 资产 ID。
    netboot: i64,
}

/// 写入密钥与配置，返回静态配置。
///
/// # Errors
///
/// 写文件或配置不合法时返回错误。
fn prepare(config: &Config) -> Outcome<Settings> {
    let dir = &config.dir;
    if dir.join("data").exists() {
        std::fs::remove_dir_all(dir.join("data"))?;
    }
    std::fs::create_dir_all(dir)?;
    for name in ["master.key", "admin.pw", "bootnet.token", "xpxe-agent"] {
        if dir.join(name).exists() {
            std::fs::remove_file(dir.join(name))?;
        }
    }
    write_secret(
        &dir.join("master.key"),
        b"MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWY=\n",
        0o400,
    )?;
    write_secret(&dir.join("admin.pw"), INITIAL_PASSWORD.as_bytes(), 0o400)?;
    write_secret(&dir.join("bootnet.token"), BOOTNET_TOKEN.as_bytes(), 0o400)?;
    write_secret(&dir.join("xpxe-agent"), b"#!/bin/sh\nexit 0\n", 0o755)?;
    let path = |name: &str| dir.join(name).display().to_string();
    let env = std::collections::BTreeMap::from([
        ("XPXE_BOOT_BASE_URL", format!("http://{}", config.boot)),
        ("XPXE_ADMIN_LISTEN", config.admin.to_string()),
        ("XPXE_BOOT_LISTEN", config.boot.to_string()),
        ("XPXE_INTERNAL_LISTEN", "127.0.0.1:0".to_owned()),
        ("XPXE_DATA_DIR", path("data")),
        ("XPXE_WEB_DIR", config.web.display().to_string()),
        ("XPXE_MASTER_KEY_FILE", path("master.key")),
        ("XPXE_ADMIN_INITIAL_PASSWORD_FILE", path("admin.pw")),
        ("XPXE_BOOTNET_TOKEN_FILE", path("bootnet.token")),
        ("XPXE_AGENT_FILE", path("xpxe-agent")),
        ("XPXE_BOOTNET_ALLOWED_IFACES", ALLOWED_IFACE.to_owned()),
    ]);
    Settings::from_lookup(|key| env.get(key).cloned())
        .map_err(|error| TestFailure::new(error.to_string()).into())
}

/// 以 bootnet 身份上报一次心跳。
///
/// # Errors
///
/// 请求失败或非 2xx 时返回错误。
pub async fn beat(app: &App, internal: SocketAddr) -> Outcome<()> {
    let heartbeat = Heartbeat {
        at: app.now(),
        version: "webui-stack".to_owned(),
        rev: 0,
        mode: "proxy".to_owned(),
        health: Health::Ok,
        detail: None,
        listening: vec![67, 69, 4011],
        counters: xpxe_domain::Counters::default(),
        probe: None,
    };
    let call = Call {
        method: "POST",
        path: "/internal/v1/bootnet/heartbeat",
        headers: vec![
            ("Authorization", format!("Bearer {BOOTNET_TOKEN}")),
            ("Content-Type", "application/json".to_owned()),
        ],
        body: serde_json::to_vec(&heartbeat)?,
    };
    let reply = send(internal, None, &call).await?;
    if (200..300).contains(&reply.status) {
        Ok(())
    } else {
        Err(format!("心跳 → {}", reply.status).into())
    }
}

/// 周期心跳，失败（控制面已停止）即退出。
async fn beat_forever(app: Arc<App>, internal: SocketAddr) {
    tokio::time::sleep(BEAT_EVERY).await;
    while beat(&app, internal).await.is_ok() {
        tokio::time::sleep(BEAT_EVERY).await;
    }
}

impl Stack {
    /// 启动控制面、镜像与心跳。
    ///
    /// # Errors
    ///
    /// 配置、监听或夹具登记失败时返回错误。
    pub async fn start(config: &Config) -> Outcome<Self> {
        let settings = prepare(config)?;
        let assets = settings.assets_dir();
        let options = Options {
            poll: Duration::from_millis(500),
            ..Options::default()
        };
        let app =
            App::open(settings, options).map_err(|error| TestFailure::new(error.to_string()))?;
        let running = server::start(app).await?;
        let mirror = mirror::serve(mirror::debian_fixture()?).await?;
        let netboot = seed::stage_netboot(&running.app(), &assets).await?;
        let shared = running.app();
        let internal = running.internal_addr();
        beat(&shared, internal).await?;
        let _beats = tokio::spawn(beat_forever(shared, internal));
        Ok(Self {
            running,
            mirror,
            netboot,
        })
    }

    /// 连接信息（不含任何口令或令牌）。
    #[must_use]
    pub fn info(&self) -> Value {
        json!({
            "admin": format!("http://{}", self.running.admin_addr()),
            "boot": format!("http://{}", self.running.boot_addr()),
            "mirror": self.mirror.to_string(),
            "netboot_asset": self.netboot,
            "allowed_iface": ALLOWED_IFACE,
        })
    }

    /// 应用状态。
    #[must_use]
    pub fn app(&self) -> Arc<App> {
        self.running.app()
    }

    /// 停止。
    pub async fn shutdown(self) {
        self.running.shutdown().await;
    }
}

/// 解析 `--key value` 参数。
fn arg(args: &[String], key: &str) -> Option<String> {
    args.windows(2).find_map(|pair| match pair {
        [name, value] if name == key => Some(value.clone()),
        _ => None,
    })
}

/// 命令行 → 栈参数（缺省：`target/webui-stack`、管理端 8088、引导端 8080、`web/dist`）。
///
/// # Errors
///
/// 地址非法时返回错误。
pub fn config_from(args: &[String], root: &Path) -> Outcome<Config> {
    Ok(Config {
        dir: arg(args, "--dir").map_or_else(|| root.join("target/webui-stack"), PathBuf::from),
        admin: arg(args, "--admin")
            .unwrap_or_else(|| "127.0.0.1:8088".to_owned())
            .parse()?,
        boot: arg(args, "--boot")
            .unwrap_or_else(|| "127.0.0.1:8080".to_owned())
            .parse()?,
        web: arg(args, "--web").map_or_else(|| root.join("web/dist"), PathBuf::from),
    })
}
