//! 静态配置（环境变量，见总体设计第 9 节）。

use std::net::SocketAddr;
use std::path::PathBuf;

use xpxe_domain::HttpUrl;

use crate::Failure;

/// 控制面静态配置。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// 管理端监听。
    pub admin_listen: SocketAddr,
    /// 引导端监听。
    pub boot_listen: SocketAddr,
    /// 内部 API 监听。
    pub internal_listen: SocketAddr,
    /// 引导地址（目标机可达，端口写全）。
    pub boot_base: HttpUrl,
    /// 数据目录。
    pub data_dir: PathBuf,
    /// `WebUI` 静态文件目录。
    pub web_dir: PathBuf,
    /// 主密钥文件。
    pub master_key_file: PathBuf,
    /// 初始管理员密码文件。
    pub admin_password_file: PathBuf,
    /// bootnet 令牌文件。
    pub bootnet_token_file: PathBuf,
    /// 管理端 TLS 证书与私钥（两者同时提供才启用 HTTPS）。
    pub tls: Option<(PathBuf, PathBuf)>,
    /// 离线模式：禁止访问官方公网源。
    pub offline: bool,
    /// 下发给目标机的 `xpxe-agent` 二进制。
    pub agent_file: PathBuf,
}

/// 解析监听地址。
///
/// # Errors
///
/// 格式不合法时返回错误。
fn listen(value: Option<String>, default: &str, name: &str) -> Result<SocketAddr, Failure> {
    let raw = value.unwrap_or_else(|| default.to_owned());
    raw.parse().map_err(|_error| {
        Failure::bad_request("BAD_SETTING", format!("{name} 不是 host:port：{raw}"))
    })
}

impl Settings {
    /// 从进程环境读取。
    ///
    /// # Errors
    ///
    /// 必填项缺失或格式不合法时返回错误。
    pub fn from_env() -> Result<Self, Failure> {
        Self::from_lookup(|key| std::env::var(key).ok())
    }

    /// 从任意查找函数读取（测试用）。
    ///
    /// # Errors
    ///
    /// 必填项缺失、格式不合法，或只提供了证书与私钥之一时返回错误。
    pub fn from_lookup<F: Fn(&str) -> Option<String>>(lookup: F) -> Result<Self, Failure> {
        let get = |key: &str| lookup(key).filter(|value| !value.trim().is_empty());
        let path = |key: &str, default: &str| {
            PathBuf::from(get(key).unwrap_or_else(|| default.to_owned()))
        };
        let base_raw = get("XPXE_BOOT_BASE_URL")
            .ok_or_else(|| Failure::bad_request("BAD_SETTING", "缺少 XPXE_BOOT_BASE_URL"))?;
        let boot_base = HttpUrl::parse(base_raw.trim_end_matches('/')).map_err(|error| {
            Failure::bad_request("BAD_SETTING", format!("XPXE_BOOT_BASE_URL：{error}"))
        })?;
        let tls = match (get("XPXE_TLS_CERT_FILE"), get("XPXE_TLS_KEY_FILE")) {
            (Some(cert), Some(key)) => Some((PathBuf::from(cert), PathBuf::from(key))),
            (None, None) => None,
            (Some(_), None) | (None, Some(_)) => {
                return Err(Failure::bad_request(
                    "BAD_SETTING",
                    "XPXE_TLS_CERT_FILE 与 XPXE_TLS_KEY_FILE 必须同时提供",
                ));
            }
        };
        Ok(Self {
            admin_listen: listen(
                get("XPXE_ADMIN_LISTEN"),
                "0.0.0.0:8088",
                "XPXE_ADMIN_LISTEN",
            )?,
            boot_listen: listen(get("XPXE_BOOT_LISTEN"), "0.0.0.0:8080", "XPXE_BOOT_LISTEN")?,
            internal_listen: listen(
                get("XPXE_INTERNAL_LISTEN"),
                "0.0.0.0:8081",
                "XPXE_INTERNAL_LISTEN",
            )?,
            boot_base,
            data_dir: path("XPXE_DATA_DIR", "/data"),
            web_dir: path("XPXE_WEB_DIR", "/web"),
            master_key_file: path("XPXE_MASTER_KEY_FILE", "/run/secrets/xpxe_master_key"),
            admin_password_file: path(
                "XPXE_ADMIN_INITIAL_PASSWORD_FILE",
                "/run/secrets/xpxe_admin_initial_password",
            ),
            bootnet_token_file: path("XPXE_BOOTNET_TOKEN_FILE", "/run/secrets/xpxe_bootnet_token"),
            tls,
            offline: get("XPXE_OFFLINE").is_some_and(|value| value == "true" || value == "1"),
            agent_file: path("XPXE_AGENT_FILE", "/usr/local/share/xpxe/xpxe-agent"),
        })
    }

    /// 资产目录。
    #[must_use]
    pub fn assets_dir(&self) -> PathBuf {
        self.data_dir.join("assets")
    }

    /// 会话产物目录。
    #[must_use]
    pub fn sessions_dir(&self) -> PathBuf {
        self.data_dir.join("cache").join("sessions")
    }

    /// Redfish ISO 缓存目录。
    #[must_use]
    pub fn iso_dir(&self) -> PathBuf {
        self.data_dir.join("cache").join("iso")
    }
}

#[cfg(test)]
mod tests;
