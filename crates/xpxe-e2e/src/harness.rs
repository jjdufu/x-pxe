//! 进程内控制面：临时数据目录、随机端口、测试时钟；附带登录与常用请求助手。

use std::collections::BTreeMap;
use std::net::{SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use crate::Outcome;
use serde_json::{Value, json};
use xpxe_control::app::{App, Options};
use xpxe_control::clock::ManualTime;
use xpxe_control::server::{self, Running};
use xpxe_control::settings::Settings;
use xpxe_domain::Timestamp;
use xpxe_test_support::{TempDataDir, TestFailure};

use crate::http::{Call, Reply, send, tls_config};
use crate::mirror;

/// 测试起始时间（与 xpxe-assets 夹具 `InRelease` 的有效期一致）。
pub const START: Timestamp = Timestamp::from_unix_seconds(1_795_478_400);
/// 初始管理员口令。
pub const INITIAL_PASSWORD: &str = "Init-Admin-Passw0rd!";
/// 改密后的管理员口令。
pub const ADMIN_PASSWORD: &str = "E2e-Admin-Passw0rd-2026!";
/// bootnet 令牌。
pub const BOOTNET_TOKEN: &str = "e2e-bootnet-token-0123456789abcdef";

/// 启动选项。
#[derive(Debug, Clone, Default)]
pub struct Launch {
    /// 管理端证书与私钥（PEM）。
    pub tls: Option<(PathBuf, PathBuf)>,
    /// 只配置证书（应启动失败）。
    pub cert_only: bool,
}

/// 管理端登录态。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Login {
    /// Cookie 头值。
    pub cookie: String,
    /// CSRF 令牌。
    pub csrf: String,
}

/// 运行中的进程内控制面。
#[derive(Debug)]
pub struct Harness {
    /// 临时数据目录（析构时删除）。
    dir: TempDataDir,
    /// 测试时钟。
    clock: Arc<ManualTime>,
    /// 控制面。
    running: Option<Running>,
    /// 管理端 TLS 连接器（配置证书时）。
    admin_tls: Option<Arc<rustls::ClientConfig>>,
    /// 模拟镜像地址。
    mirror: SocketAddr,
}

/// 取一个空闲端口（绑定后立即释放）。
///
/// # Errors
///
/// 绑定失败时返回错误。
fn free_port() -> Outcome<u16> {
    Ok(TcpListener::bind("127.0.0.1:0")?.local_addr()?.port())
}

/// 写文件并设置权限。
///
/// # Errors
///
/// 写入失败时返回错误。
fn write_secret(path: &Path, content: &[u8], mode: u32) -> Outcome<()> {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::write(path, content)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))?;
    Ok(())
}

/// 环境变量表 → 静态配置。
///
/// # Errors
///
/// 配置不合法时返回错误。
fn settings_of(map: &BTreeMap<&str, String>) -> Outcome<Settings> {
    Settings::from_lookup(|key| map.get(key).cloned())
        .map_err(|error| TestFailure::new(error.to_string()).into())
}

/// 测试 CA 签发的 127.0.0.1 证书目录（xpxe-assets 夹具）。
#[must_use]
pub fn tls_fixture() -> (PathBuf, PathBuf, PathBuf) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../xpxe-assets/tests/fixtures/tls");
    (
        dir.join("server.pem"),
        dir.join("server.key"),
        dir.join("ca.pem"),
    )
}

/// 准备数据目录与环境变量表。
///
/// # Errors
///
/// 写文件失败时返回错误。
pub fn prepare(dir: &Path, launch: &Launch) -> Outcome<BTreeMap<&'static str, String>> {
    let boot_port = free_port()?;
    let secret = |name: &str| dir.join(name).display().to_string();
    write_secret(
        &dir.join("master.key"),
        b"MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWY=\n",
        0o400,
    )?;
    write_secret(&dir.join("admin.pw"), INITIAL_PASSWORD.as_bytes(), 0o400)?;
    write_secret(&dir.join("bootnet.token"), BOOTNET_TOKEN.as_bytes(), 0o400)?;
    write_secret(&dir.join("xpxe-agent"), b"#!/bin/sh\nexit 0\n", 0o755)?;
    std::fs::create_dir_all(dir.join("web"))?;
    std::fs::write(
        dir.join("web/index.html"),
        "<!doctype html><title>x-pxe</title>",
    )?;
    let mut map = BTreeMap::from([
        (
            "XPXE_BOOT_BASE_URL",
            format!("http://127.0.0.1:{boot_port}"),
        ),
        ("XPXE_ADMIN_LISTEN", "127.0.0.1:0".to_owned()),
        ("XPXE_BOOT_LISTEN", format!("127.0.0.1:{boot_port}")),
        ("XPXE_INTERNAL_LISTEN", "127.0.0.1:0".to_owned()),
        ("XPXE_DATA_DIR", secret("data")),
        ("XPXE_WEB_DIR", secret("web")),
        ("XPXE_MASTER_KEY_FILE", secret("master.key")),
        ("XPXE_ADMIN_INITIAL_PASSWORD_FILE", secret("admin.pw")),
        ("XPXE_BOOTNET_TOKEN_FILE", secret("bootnet.token")),
        ("XPXE_AGENT_FILE", secret("xpxe-agent")),
    ]);
    if let Some((cert, key)) = &launch.tls {
        let _cert = map.insert("XPXE_TLS_CERT_FILE", cert.display().to_string());
        if !launch.cert_only {
            let _key = map.insert("XPXE_TLS_KEY_FILE", key.display().to_string());
        }
    }
    Ok(map)
}

/// 构造配置（供启动失败断言使用）。
///
/// # Errors
///
/// 配置不合法时返回错误。
pub fn settings_for(dir: &Path, launch: &Launch) -> Outcome<Settings> {
    settings_of(&prepare(dir, launch)?)
}

impl Harness {
    /// 启动。
    ///
    /// # Errors
    ///
    /// 配置、数据目录或监听失败时返回错误。
    pub async fn start(launch: Launch) -> Outcome<Self> {
        let dir = TempDataDir::new()?;
        let settings = settings_for(dir.path(), &launch)?;
        let clock = Arc::new(ManualTime::starting_at(START));
        let shared: Arc<dyn xpxe_control::clock::Clock> = Arc::<ManualTime>::clone(&clock);
        let options = Options {
            clock: shared,
            poll: Duration::from_millis(50),
            ..Options::default()
        };
        let app =
            App::open(settings, options).map_err(|error| TestFailure::new(error.to_string()))?;
        let running = server::start(app).await?;
        let admin_tls = match &launch.tls {
            Some(_) => Some(tls_config(&std::fs::read(tls_fixture().2)?)?),
            None => None,
        };
        let mirror = mirror::serve(mirror::debian_fixture()?).await?;
        Ok(Self {
            dir,
            clock,
            running: Some(running),
            admin_tls,
            mirror,
        })
    }

    /// 控制面句柄。
    ///
    /// # Errors
    ///
    /// 已停止时返回错误。
    pub fn running(&self) -> Outcome<&Running> {
        self.running
            .as_ref()
            .ok_or_else(|| TestFailure::new("控制面已停止").into())
    }

    /// 应用状态。
    ///
    /// # Errors
    ///
    /// 已停止时返回错误。
    pub fn app(&self) -> Outcome<Arc<App>> {
        Ok(self.running()?.app())
    }

    /// 数据目录。
    #[must_use]
    pub fn data_dir(&self) -> PathBuf {
        self.dir.path().join("data")
    }

    /// 模拟镜像地址。
    #[must_use]
    pub const fn mirror(&self) -> SocketAddr {
        self.mirror
    }

    /// 临时目录中的文件（主密钥 `master.key` 等）。
    #[must_use]
    pub fn file(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    /// bootnet 令牌文件。
    #[must_use]
    pub fn token_file(&self) -> PathBuf {
        self.dir.path().join("bootnet.token")
    }

    /// 停止控制面。
    pub async fn stop(&mut self) {
        if let Some(running) = self.running.take() {
            running.shutdown().await;
        }
    }

    /// 拨动时钟并唤醒 worker，稍候让定时器运行。
    ///
    /// # Errors
    ///
    /// 已停止时返回错误。
    pub async fn advance(&self, secs: u64) -> Outcome<()> {
        self.clock.advance(Duration::from_secs(secs));
        self.app()?.wake();
        tokio::time::sleep(Duration::from_millis(200)).await;
        Ok(())
    }

    /// 管理端请求。
    ///
    /// # Errors
    ///
    /// 连接失败时返回错误。
    pub async fn admin(
        &self,
        method: &str,
        path: &str,
        login: Option<&Login>,
        body: Option<&Value>,
    ) -> Outcome<Reply> {
        let mut headers = vec![("Content-Type", "application/json".to_owned())];
        if let Some(session) = login {
            headers.push(("Cookie", session.cookie.clone()));
            headers.push(("X-CSRF-Token", session.csrf.clone()));
        }
        let payload = body.map_or_else(Vec::new, |value| value.to_string().into_bytes());
        let call = Call {
            method,
            path,
            headers,
            body: payload,
        };
        send(self.running()?.admin_addr(), self.admin_tls.as_ref(), &call).await
    }

    /// 任意请求发往管理端（自带请求头）。
    ///
    /// # Errors
    ///
    /// 连接失败时返回错误。
    pub async fn admin_raw(&self, call: &Call<'_>) -> Outcome<Reply> {
        send(self.running()?.admin_addr(), self.admin_tls.as_ref(), call).await
    }

    /// 引导端请求。
    ///
    /// # Errors
    ///
    /// 连接失败时返回错误。
    pub async fn boot(&self, method: &str, path: &str, body: &[u8]) -> Outcome<Reply> {
        let headers = vec![(
            "Content-Type",
            if body.first() == Some(&b'{') {
                "application/json"
            } else {
                "text/plain"
            }
            .to_owned(),
        )];
        send(
            self.running()?.boot_addr(),
            None,
            &Call {
                method,
                path,
                headers,
                body: body.to_vec(),
            },
        )
        .await
    }

    /// 内部 API 请求（带 bootnet 令牌）。
    ///
    /// # Errors
    ///
    /// 连接失败时返回错误。
    pub async fn internal(&self, method: &str, path: &str, body: Option<&Value>) -> Outcome<Reply> {
        let headers = vec![
            ("Authorization", format!("Bearer {BOOTNET_TOKEN}")),
            ("Content-Type", "application/json".to_owned()),
        ];
        let payload = body.map_or_else(Vec::new, |value| value.to_string().into_bytes());
        send(
            self.running()?.internal_addr(),
            None,
            &Call {
                method,
                path,
                headers,
                body: payload,
            },
        )
        .await
    }

    /// 登录。
    ///
    /// # Errors
    ///
    /// 登录失败时返回错误。
    pub async fn login(&self, username: &str, password: &str) -> Outcome<Login> {
        let reply = self
            .admin(
                "POST",
                "/api/v1/auth/login",
                None,
                Some(&json!({ "username": username, "password": password })),
            )
            .await?;
        if reply.status != 200 {
            return Err(format!("登录失败 {}：{}", reply.status, reply.text()).into());
        }
        let cookie = reply
            .header("set-cookie")
            .and_then(|value| value.split(';').next())
            .ok_or_else(|| TestFailure::new("缺少 Set-Cookie"))?
            .to_owned();
        let csrf = reply
            .json()?
            .get("csrf_token")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        Ok(Login { cookie, csrf })
    }

    /// 初始管理员登录并改密，返回改密后的新会话。
    ///
    /// # Errors
    ///
    /// 任一步失败时返回错误。
    pub async fn admin_login(&self) -> Outcome<Login> {
        let first = self.login("admin", INITIAL_PASSWORD).await?;
        let blocked = self
            .admin("GET", "/api/v1/nodes", Some(&first), None)
            .await?;
        if blocked.status != 403 {
            return Err(format!("改密前应被拒绝，实际 {}", blocked.status).into());
        }
        let changed = self
            .admin(
                "POST",
                "/api/v1/me/password",
                Some(&first),
                Some(&json!({ "old_password": INITIAL_PASSWORD, "new_password": ADMIN_PASSWORD })),
            )
            .await?;
        if changed.status != 200 && changed.status != 204 {
            return Err(format!("改密失败 {}：{}", changed.status, changed.text()).into());
        }
        self.login("admin", ADMIN_PASSWORD).await
    }

    /// 期望 2xx 并返回 JSON。
    ///
    /// # Errors
    ///
    /// 非 2xx 或非 JSON 时返回错误。
    pub async fn ok(
        &self,
        method: &str,
        path: &str,
        login: &Login,
        body: Option<&Value>,
    ) -> Outcome<Value> {
        let reply = self.admin(method, path, Some(login), body).await?;
        if !(200..300).contains(&reply.status) {
            return Err(format!("{method} {path} → {}：{}", reply.status, reply.text()).into());
        }
        if reply.body.is_empty() {
            Ok(Value::Null)
        } else {
            reply.json()
        }
    }

    /// 节点当前状态名。
    ///
    /// # Errors
    ///
    /// 请求失败时返回错误。
    pub async fn state(&self, login: &Login, node: i64) -> Outcome<String> {
        let detail = self
            .ok("GET", &format!("/api/v1/nodes/{node}"), login, None)
            .await?;
        Ok(detail
            .pointer("/node/state")
            .or_else(|| detail.get("state"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned())
    }

    /// 轮询直到节点进入期望状态（最多 5 秒）。
    ///
    /// # Errors
    ///
    /// 超时时返回错误。
    pub async fn wait_state(&self, login: &Login, node: i64, want: &str) -> Outcome<()> {
        let mut last = String::new();
        for _attempt in 0_u32..50 {
            last = self.state(login, node).await?;
            if last == want {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        Err(format!("节点 {node} 未进入 {want}，当前 {last}").into())
    }
}
