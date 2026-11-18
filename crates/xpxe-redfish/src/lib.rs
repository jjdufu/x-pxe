//! x-pxe 的 Redfish 客户端：证书指纹固定、会话与资源发现、厂商画像、虚拟介质部署流程。
//!
//! - [`probe`]：接受任意证书完成握手，只请求无需认证的 `GET /redfish/v1`，返回证书指纹与服务信息，
//!   不发送任何凭据。
//! - [`Session::open`]：只接受终端证书 SHA-256 等于已固定值的连接；优先 `SessionService` 令牌，
//!   不支持时回落 Basic 认证。
//! - [`Session::discover`] / [`Session::deploy`]：定位系统与 CD 虚拟介质，按厂商画像插入 ISO、
//!   设置一次性 UEFI CD 启动并重启。
//!
//! 本 crate 不使用 `unsafe`；`rustls::client::danger` 只是 rustls 的模块名。
#![doc(test(attr(deny(warnings), forbid(unsafe_code))))]

mod client;
mod pin;
mod profile;
mod settings;
mod transport;
mod wire;
mod x509;

pub use client::{DeployEvent, PowerAction, ProbeReport, Session, Target, probe};
pub use pin::Fingerprint;
pub use profile::{
    BootOverrideStyle, InsertFields, ProfileId, ResetPolicy, ResetType, TransferFields,
    VendorProfile, VmediaLocation,
};
pub use settings::{BmcConfig, Credentials, Endpoint, Timing};
pub use x509::CertSummary;

/// Redfish 操作失败。`code` 为稳定错误码，`message` 为给人看的说明（含 BMC 的 `ExtendedInfo`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedfishError {
    /// 稳定错误码，例如 `cert_changed`、`unauthorized`、`legacy_boot_mode`。
    code: &'static str,
    /// 说明。
    message: String,
}

impl RedfishError {
    /// 构造错误。
    #[must_use]
    pub const fn new(code: &'static str, message: String) -> Self {
        Self { code, message }
    }

    /// 稳定错误码。
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

impl std::fmt::Display for RedfishError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for RedfishError {}

/// 本 crate 的结果类型。
pub type RedfishResult<T> = Result<T, RedfishError>;
