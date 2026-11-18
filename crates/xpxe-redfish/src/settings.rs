//! BMC 连接参数：地址、凭据（不在 `Debug` 中泄露）、固定指纹、画像覆盖与轮询时序。

use std::time::Duration;

use crate::pin::Fingerprint;
use crate::profile::ProfileId;
use crate::{RedfishError, RedfishResult};

/// BMC 地址：`https://<主机>[:端口]`，不带路径。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint(String);

impl Endpoint {
    /// 解析地址；末尾斜杠会去掉。
    ///
    /// # Errors
    ///
    /// 不是 `https://`、带路径/查询或含空白时返回 `bad_endpoint`。
    pub fn parse(raw: &str) -> RedfishResult<Self> {
        let text = raw.trim().trim_end_matches('/');
        let host = text.strip_prefix("https://").ok_or_else(|| {
            RedfishError::new("bad_endpoint", "BMC 地址必须以 https:// 开头".to_owned())
        })?;
        if host.is_empty()
            || host
                .chars()
                .any(|ch| ch.is_whitespace() || matches!(ch, '/' | '?' | '#' | '@'))
        {
            return Err(RedfishError::new(
                "bad_endpoint",
                format!("BMC 地址不合法：{raw}"),
            ));
        }
        Ok(Self(text.to_owned()))
    }

    /// 规范文本。
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// BMC 凭据。`Debug` 只显示用户名。
#[derive(Clone, PartialEq, Eq)]
pub struct Credentials {
    /// 用户名。
    username: String,
    /// 密码。
    password: String,
}

impl Credentials {
    /// 构造凭据。
    #[must_use]
    pub const fn new(username: String, password: String) -> Self {
        Self { username, password }
    }

    /// 用户名。
    #[must_use]
    pub fn username(&self) -> &str {
        &self.username
    }

    /// 密码（只在建立会话或 Basic 认证时使用）。
    #[must_use]
    pub fn password(&self) -> &str {
        &self.password
    }
}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credentials")
            .field("username", &self.username)
            .field("password", &"<redacted>")
            .finish()
    }
}

/// 轮询时序。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timing {
    /// 单次 HTTP 请求超时。
    pub request: Duration,
    /// 等待 `Inserted = true` 的上限（计划：60 秒）。
    pub insert: Duration,
    /// 等待 202 任务完成的上限。
    pub task: Duration,
    /// 轮询间隔。
    pub poll: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            request: Duration::from_secs(30),
            insert: Duration::from_secs(60),
            task: Duration::from_secs(120),
            poll: Duration::from_secs(1),
        }
    }
}

/// 一台 BMC 的完整连接配置。
#[derive(Debug, Clone)]
pub struct BmcConfig {
    /// 地址。
    pub endpoint: Endpoint,
    /// 凭据。
    pub credentials: Credentials,
    /// 管理员确认过的证书指纹。
    pub pin: Fingerprint,
    /// 手动指定的厂商画像（`None` 时按 `ServiceRoot.Vendor` 自动选择）。
    pub profile: Option<ProfileId>,
    /// 多系统 BMC 上使用的 `Systems` 成员 ID。
    pub system_id: Option<String>,
    /// 轮询时序。
    pub timing: Timing,
}
