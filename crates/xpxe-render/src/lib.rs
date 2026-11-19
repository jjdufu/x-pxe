//! 纯函数渲染：相同输入得到逐字节相同的 iPXE、内核参数与 preseed。
#![doc(test(attr(deny(warnings), forbid(unsafe_code))))]

mod cmdline;
pub mod dhcp_snippets;
mod disk;
mod filters;
mod hashpw;
pub mod init;
mod ipxe;
pub mod manual;
mod md5sum;
mod preseed;
pub mod redfish_boot;
mod request;
mod template;

pub use hashpw::hash_password;
pub use request::{
    ArmedState, RenderRequest, RenderRequestParts, RenderedInstall, RenderedParts, RequestDigests,
    RequestTokens, SHA256_LEN,
};

use xpxe_assets::parse_public_key;
use xpxe_domain::Profile;

use crate::cmdline::build_cmdline;
use crate::disk::render_disk_artifacts;
use crate::ipxe::render_ipxe;
use crate::preseed::render_preseed;

/// 渲染失败。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderError {
    /// 稳定错误码。
    code: &'static str,
    /// 说明。
    message: String,
}

impl RenderError {
    /// 构造错误。
    #[must_use]
    pub const fn new(code: &'static str, message: String) -> Self {
        Self { code, message }
    }

    /// 错误码。
    #[must_use]
    pub const fn code(&self) -> &'static str {
        self.code
    }

    /// 说明。
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl std::fmt::Display for RenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for RenderError {}

/// 将 Profile、节点与会话渲染为安装器输入。
///
/// # Errors
///
/// 校验失败、模板失败、内核参数超长或额外仓库公钥无法解析时返回错误。
pub fn render(request: &RenderRequest) -> Result<RenderedInstall, RenderError> {
    request.check()?;
    validate_repo_keys(request.profile())?;
    let preseed = render_preseed(request)?;
    let preseed_md5 = md5_hex(preseed.as_bytes());
    let cmdline = build_cmdline(request, &preseed_md5)?;
    let boot_ipxe = render_ipxe(request, &cmdline)?;
    let disks = render_disk_artifacts(request)?;
    Ok(RenderedInstall::from_parts(RenderedParts {
        boot_ipxe,
        cmdline,
        preseed,
        preseed_md5,
        disk_select: disks.disk_select,
        partman_early: disks.partman_early,
        late: disks.late,
        firstboot_unit: disks.firstboot_unit,
    }))
}

/// 用 `xpxe-assets` 解析每个额外仓库公钥。
///
/// # Errors
///
/// 公钥无法解析时返回错误。
fn validate_repo_keys(profile: &Profile) -> Result<(), RenderError> {
    for (index, repo) in profile.extra_repos().iter().enumerate() {
        let _info = parse_public_key(repo.key.as_str()).map_err(|error| {
            RenderError::new(
                "bad_repo_key",
                format!("extra_repos/{index} 公钥无法解析：{error}"),
            )
        })?;
    }
    Ok(())
}

/// 计算 MD5 十六进制（供 d-i `preseed/url/checksum`）。
fn md5_hex(bytes: &[u8]) -> String {
    md5sum::hex_digest(bytes)
}

#[cfg(test)]
mod tests;
