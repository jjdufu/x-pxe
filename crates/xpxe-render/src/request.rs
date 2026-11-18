//! 渲染输入与输出。

use serde::Serialize;
use xpxe_domain::{BootToken, EventId, HttpUrl, NodeSpec, Profile};

use crate::RenderError;

/// SHA-256 十六进制长度。
pub const SHA256_LEN: usize = 64;

/// 一次渲染所需的会话与文档。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RenderRequest {
    /// 装机 Profile。
    profile: Profile,
    /// 节点规格。
    node: NodeSpec,
    /// HTTP 根，例如 `http://10.77.0.1:8080`。
    boot_base: HttpUrl,
    /// 引导令牌。
    boot_token: BootToken,
    /// Agent / plan 令牌。
    agent_token: BootToken,
    /// 会话短标识（用于 iPXE 提示）。
    session_short: String,
    /// 安装事件 ID（由调用方生成，保证纯函数）。
    event_id: EventId,
    /// agent 文件 SHA-256（小写十六进制）。
    agent_sha256: String,
    /// plan.json SHA-256（小写十六进制）。
    plan_sha256: String,
    /// 节点是否处于 Armed；否则入口脚本只打印原因并退出。
    armed: bool,
    /// 非 Armed 时打印的原因。
    not_armed_reason: String,
}

impl RenderRequest {
    /// 构造请求（不做跨字段以外的校验，见 [`Self::check`]）。
    #[must_use]
    pub fn from_parts(parts: RenderRequestParts) -> Self {
        Self {
            profile: parts.profile,
            node: parts.node,
            boot_base: parts.boot_base,
            boot_token: parts.tokens.boot_token,
            agent_token: parts.tokens.agent_token,
            session_short: parts.tokens.session_short,
            event_id: parts.tokens.event_id,
            agent_sha256: parts.digests.agent_sha256,
            plan_sha256: parts.digests.plan_sha256,
            armed: parts.armed.armed,
            not_armed_reason: parts.armed.reason,
        }
    }

    /// 校验哈希与短标识。
    ///
    /// # Errors
    ///
    /// 会话短标识为空或哈希不是 64 位小写十六进制时返回错误。
    pub fn check(&self) -> Result<(), RenderError> {
        if self.session_short.is_empty() || self.session_short.len() > 64 {
            return Err(RenderError::new(
                "bad_session",
                "会话短标识长度不合法".to_owned(),
            ));
        }
        if self.session_short.chars().any(char::is_control) {
            return Err(RenderError::new(
                "bad_session",
                "会话短标识含控制字符".to_owned(),
            ));
        }
        check_sha256("agent_sha256", &self.agent_sha256)?;
        check_sha256("plan_sha256", &self.plan_sha256)?;
        Ok(())
    }

    /// Profile。
    #[must_use]
    pub const fn profile(&self) -> &Profile {
        &self.profile
    }

    /// 节点。
    #[must_use]
    pub const fn node(&self) -> &NodeSpec {
        &self.node
    }

    /// HTTP 根。
    #[must_use]
    pub const fn boot_base(&self) -> &HttpUrl {
        &self.boot_base
    }

    /// 引导令牌。
    #[must_use]
    pub const fn boot_token(&self) -> &BootToken {
        &self.boot_token
    }

    /// Agent 令牌。
    #[must_use]
    pub const fn agent_token(&self) -> &BootToken {
        &self.agent_token
    }

    /// 会话短标识。
    #[must_use]
    pub fn session_short(&self) -> &str {
        &self.session_short
    }

    /// 事件 ID。
    #[must_use]
    pub const fn event_id(&self) -> &EventId {
        &self.event_id
    }

    /// agent 哈希。
    #[must_use]
    pub fn agent_sha256(&self) -> &str {
        &self.agent_sha256
    }

    /// plan 哈希。
    #[must_use]
    pub fn plan_sha256(&self) -> &str {
        &self.plan_sha256
    }

    /// 是否 Armed。
    #[must_use]
    pub const fn armed(&self) -> bool {
        self.armed
    }

    /// 非 Armed 原因。
    #[must_use]
    pub fn not_armed_reason(&self) -> &str {
        &self.not_armed_reason
    }

    /// 会话资源前缀，例如 `http://host/p/token`。
    #[must_use]
    pub fn session_base(&self) -> String {
        format!("{}/p/{}", self.boot_base.as_str(), self.boot_token.as_str())
    }

    /// preseed URL。
    #[must_use]
    pub fn preseed_url(&self) -> String {
        format!("{}/preseed.cfg", self.session_base())
    }
}

/// 构造 [`RenderRequest`] 的字段组。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderRequestParts {
    /// Profile。
    pub profile: Profile,
    /// 节点。
    pub node: NodeSpec,
    /// HTTP 根。
    pub boot_base: HttpUrl,
    /// 令牌。
    pub tokens: RequestTokens,
    /// 摘要。
    pub digests: RequestDigests,
    /// Armed 状态。
    pub armed: ArmedState,
}

/// 会话令牌组。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestTokens {
    /// 引导令牌。
    pub boot_token: BootToken,
    /// Agent 令牌。
    pub agent_token: BootToken,
    /// 会话短标识。
    pub session_short: String,
    /// 事件 ID。
    pub event_id: EventId,
}

/// 文件摘要组。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestDigests {
    /// agent SHA-256。
    pub agent_sha256: String,
    /// plan SHA-256。
    pub plan_sha256: String,
}

/// Armed 状态与原因。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArmedState {
    /// 是否 Armed。
    pub armed: bool,
    /// 非 Armed 时的原因。
    pub reason: String,
}

impl ArmedState {
    /// 已武装。
    #[must_use]
    pub const fn armed() -> Self {
        Self {
            armed: true,
            reason: String::new(),
        }
    }

    /// 未武装。
    #[must_use]
    pub const fn not_armed(reason: String) -> Self {
        Self {
            armed: false,
            reason,
        }
    }
}

/// 校验 64 位小写十六进制。
///
/// # Errors
///
/// 长度或字符不合法时返回错误。
fn check_sha256(name: &str, value: &str) -> Result<(), RenderError> {
    let ok = value.len() == SHA256_LEN
        && value
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'));
    if ok {
        Ok(())
    } else {
        Err(RenderError::new(
            "bad_digest",
            format!("{name} 不是 64 位小写十六进制"),
        ))
    }
}

/// 渲染产物。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedInstall {
    /// iPXE 脚本。
    boot_ipxe: String,
    /// 内核参数（单行）。
    cmdline: String,
    /// preseed.cfg。
    preseed: String,
    /// preseed 的 MD5（小写十六进制）。
    preseed_md5: String,
    /// 选盘脚本。
    disk_select: String,
    /// partman early 钩子。
    partman_early: String,
    /// late 钩子。
    late: String,
    /// firstboot 单元。
    firstboot_unit: String,
}

/// 构造 [`RenderedInstall`] 的字段组。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedParts {
    /// iPXE。
    pub boot_ipxe: String,
    /// 内核参数。
    pub cmdline: String,
    /// preseed。
    pub preseed: String,
    /// MD5。
    pub preseed_md5: String,
    /// 选盘脚本。
    pub disk_select: String,
    /// partman 钩子。
    pub partman_early: String,
    /// late 钩子。
    pub late: String,
    /// firstboot 单元。
    pub firstboot_unit: String,
}

impl RenderedInstall {
    /// 由字段组组装。
    #[must_use]
    pub fn from_parts(parts: RenderedParts) -> Self {
        Self {
            boot_ipxe: parts.boot_ipxe,
            cmdline: parts.cmdline,
            preseed: parts.preseed,
            preseed_md5: parts.preseed_md5,
            disk_select: parts.disk_select,
            partman_early: parts.partman_early,
            late: parts.late,
            firstboot_unit: parts.firstboot_unit,
        }
    }

    /// iPXE 脚本。
    #[must_use]
    pub fn boot_ipxe(&self) -> &str {
        &self.boot_ipxe
    }

    /// 内核参数。
    #[must_use]
    pub fn cmdline(&self) -> &str {
        &self.cmdline
    }

    /// preseed。
    #[must_use]
    pub fn preseed(&self) -> &str {
        &self.preseed
    }

    /// preseed MD5。
    #[must_use]
    pub fn preseed_md5(&self) -> &str {
        &self.preseed_md5
    }

    /// 选盘脚本。
    #[must_use]
    pub fn disk_select(&self) -> &str {
        &self.disk_select
    }

    /// partman early 钩子。
    #[must_use]
    pub fn partman_early(&self) -> &str {
        &self.partman_early
    }

    /// late 钩子。
    #[must_use]
    pub fn late(&self) -> &str {
        &self.late
    }

    /// firstboot 单元。
    #[must_use]
    pub fn firstboot_unit(&self) -> &str {
        &self.firstboot_unit
    }
}
