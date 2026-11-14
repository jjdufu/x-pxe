//! DHCP 层引导决策与 iPXE 入口决策。

use std::net::Ipv4Addr;

use serde::{Deserialize, Serialize};

use crate::lifecycle::{NodeRuntime, NodeState, SessionId, Timestamp};
use crate::option93::ClientArch;
use crate::value::{BootToken, HttpUrl, MacAddr, ParseError};

/// UEFI 阶段使用的 iPXE 固件文件。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UefiFirmware {
    /// `ipxe.efi`。
    IpxeEfi,
    /// `snponly.efi`。
    SnpOnlyEfi,
}

/// TFTP 引导文件名。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FirmwareName {
    /// BIOS UNDI：`undionly.kpxe`。
    UndionlyKpxe,
    /// UEFI：`ipxe.efi`。
    IpxeEfi,
    /// UEFI SNP：`snponly.efi`。
    SnpOnlyEfi,
}

impl FirmwareName {
    /// 文件名。
    ///
    /// # Errors
    ///
    /// 失败时返回对应错误。
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UndionlyKpxe => "undionly.kpxe",
            Self::IpxeEfi => "ipxe.efi",
            Self::SnpOnlyEfi => "snponly.efi",
        }
    }

    /// 由 [`UefiFirmware`] 映射。
    #[must_use]
    pub const fn from_uefi(firmware: UefiFirmware) -> Self {
        match firmware {
            UefiFirmware::IpxeEfi => Self::IpxeEfi,
            UefiFirmware::SnpOnlyEfi => Self::SnpOnlyEfi,
        }
    }
}

/// `ProxyDHCP` 应答规则。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DhcpRules {
    /// UEFI 使用的固件。
    uefi_firmware: UefiFirmware,
    /// 是否应答 UEFI HTTP Boot。
    http_boot_enabled: bool,
    /// 是否同时镜像写入选项 66/67。由 bootnet 编码时使用，决策本身不读取。
    mirror_bootfile_options: bool,
}

impl DhcpRules {
    /// 默认：`ipxe.efi`，启用 HTTP Boot，不镜像 66/67。
    #[must_use]
    pub const fn standard() -> Self {
        Self {
            uefi_firmware: UefiFirmware::IpxeEfi,
            http_boot_enabled: true,
            mirror_bootfile_options: false,
        }
    }

    /// 指定 UEFI 固件，其余同 [`Self::standard`]。
    #[must_use]
    pub const fn with_uefi_firmware(self, uefi_firmware: UefiFirmware) -> Self {
        Self {
            uefi_firmware,
            http_boot_enabled: self.http_boot_enabled,
            mirror_bootfile_options: self.mirror_bootfile_options,
        }
    }

    /// 设置是否应答 HTTP Boot。
    #[must_use]
    pub const fn with_http_boot(self, http_boot_enabled: bool) -> Self {
        Self {
            uefi_firmware: self.uefi_firmware,
            http_boot_enabled,
            mirror_bootfile_options: self.mirror_bootfile_options,
        }
    }

    /// 设置是否镜像选项 66/67。
    #[must_use]
    pub const fn with_mirror_bootfile(self, mirror_bootfile_options: bool) -> Self {
        Self {
            uefi_firmware: self.uefi_firmware,
            http_boot_enabled: self.http_boot_enabled,
            mirror_bootfile_options,
        }
    }

    /// UEFI 固件。
    #[must_use]
    pub const fn uefi_firmware(self) -> UefiFirmware {
        self.uefi_firmware
    }

    /// 是否启用 HTTP Boot。
    #[must_use]
    pub const fn http_boot_enabled(self) -> bool {
        self.http_boot_enabled
    }

    /// 是否镜像选项 66/67。
    #[must_use]
    pub const fn mirror_bootfile_options(self) -> bool {
        self.mirror_bootfile_options
    }
}

/// 一条已武装节点的应答指令。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BootDirective {
    /// 目标 MAC。
    mac: MacAddr,
    /// 装机会话。
    session: SessionId,
    /// 引导令牌，用于拼脚本 URL。
    token: BootToken,
    /// 过期时间。
    expires_at: Timestamp,
}

impl BootDirective {
    /// 构造指令。
    #[must_use]
    pub const fn new(
        mac: MacAddr,
        session: SessionId,
        token: BootToken,
        expires_at: Timestamp,
    ) -> Self {
        Self {
            mac,
            session,
            token,
            expires_at,
        }
    }

    /// MAC。
    #[must_use]
    pub const fn mac(&self) -> MacAddr {
        self.mac
    }

    /// 会话。
    #[must_use]
    pub const fn session(&self) -> &SessionId {
        &self.session
    }

    /// 令牌。
    #[must_use]
    pub const fn token(&self) -> &BootToken {
        &self.token
    }

    /// 过期时间。
    #[must_use]
    pub const fn expires_at(&self) -> Timestamp {
        self.expires_at
    }
}

/// 当前应应答的指令表。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirectiveTable {
    /// 引导服务基址，例如 `http://192.168.2.3:8080`。
    boot_base: HttpUrl,
    /// 已武装节点。
    entries: Vec<BootDirective>,
}

impl DirectiveTable {
    /// 构造指令表。
    #[must_use]
    pub const fn new(boot_base: HttpUrl, entries: Vec<BootDirective>) -> Self {
        Self { boot_base, entries }
    }

    /// 基址。
    #[must_use]
    pub const fn boot_base(&self) -> &HttpUrl {
        &self.boot_base
    }

    /// 指令列表。
    #[must_use]
    pub fn entries(&self) -> &[BootDirective] {
        &self.entries
    }

    /// 按 MAC 查找。
    #[must_use]
    pub fn find(&self, mac: MacAddr) -> Option<&BootDirective> {
        self.entries.iter().find(|entry| entry.mac == mac)
    }
}

/// 从报文提取的、决策所需的请求视图。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DhcpRequestView {
    /// 客户端 MAC。
    pub mac: MacAddr,
    /// 中继地址，非 0 表示经过中继。
    pub giaddr: Ipv4Addr,
    /// 选项 60。
    pub vendor_class: Option<String>,
    /// 选项 77。
    pub user_class: Option<String>,
    /// 选项 93。
    pub arch: Option<u16>,
    /// 是否携带选项 175。
    pub has_opt175: bool,
}

impl DhcpRequestView {
    /// 是否为 iPXE：选项 77 为 `iPXE` 或携带选项 175。
    #[must_use]
    pub fn is_ipxe(&self) -> bool {
        self.has_opt175 || self.user_class.as_deref() == Some("iPXE")
    }

    /// 映射后的客户端架构。
    #[must_use]
    pub const fn client_arch(&self) -> ClientArch {
        ClientArch::from_option93(self.arch)
    }
}

/// 引导目标。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum BootTarget {
    /// 经 TFTP 加载固件。
    Tftp(FirmwareName),
    /// HTTP URL（iPXE 脚本或 UEFI HTTP Boot 固件）。
    Http(HttpUrl),
}

/// 一次应向客户端发出的引导要约。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootOffer {
    /// 目标 MAC。
    pub mac: MacAddr,
    /// 会话。
    pub session: SessionId,
    /// 引导目标。
    pub target: BootTarget,
    /// 应答的选项 60 应写 `HTTPClient`（仅 UEFI HTTP Boot）。
    pub http_client_class: bool,
}

/// 不应答的原因，写入观测。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IgnoreReason {
    /// 选项 60 不以 `PXEClient` 或 `HTTPClient` 开头。
    NotPxe,
    /// `giaddr ≠ 0`。
    Relayed,
    /// MAC 不在指令表。
    NotArmed,
    /// 指令已过期。
    Expired,
    /// 不支持的架构。
    UnsupportedArch,
    /// HTTP Boot 已关闭。
    HttpBootDisabled,
}

/// DHCP 层决策结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DhcpAnswer {
    /// 不应答。
    Ignore(Box<IgnoreReason>),
    /// 应交由 `xpxe-dhcp` 编码的要约。
    Offer(Box<BootOffer>),
}

/// iPXE 入口 `/boot/mac/{mac}` 的决策。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum IpxeEntry {
    /// 节点处于 `Armed`，链式加载本会话脚本。
    Chain(SessionId),
    /// 其他状态一律 `exit`。
    Exit(NodeState),
}

/// 对一次 DHCP 请求作出应答决策。
#[must_use]
pub fn answer(
    req: &DhcpRequestView,
    table: &DirectiveTable,
    rules: DhcpRules,
    now: Timestamp,
) -> DhcpAnswer {
    if let Some(reason) = ignore_reason(req, table, now) {
        return DhcpAnswer::Ignore(Box::new(reason));
    }
    match build_target(req, table, rules) {
        Ok(offer) => DhcpAnswer::Offer(Box::new(offer)),
        Err(reason) => DhcpAnswer::Ignore(Box::new(reason)),
    }
}

/// 判定顺序的前三步：中继、非 PXE、未武装/过期。
fn ignore_reason(
    req: &DhcpRequestView,
    table: &DirectiveTable,
    now: Timestamp,
) -> Option<IgnoreReason> {
    if req.giaddr != Ipv4Addr::UNSPECIFIED {
        return Some(IgnoreReason::Relayed);
    }
    if !is_pxe_or_http_client(req.vendor_class.as_deref()) {
        return Some(IgnoreReason::NotPxe);
    }
    match table.find(req.mac) {
        None => Some(IgnoreReason::NotArmed),
        Some(entry) if entry.expires_at < now => Some(IgnoreReason::Expired),
        Some(_) => None,
    }
}

/// 选项 60 是否以 `PXEClient` 或 `HTTPClient` 开头。
fn is_pxe_or_http_client(vendor: Option<&str>) -> bool {
    vendor.is_some_and(|class| class.starts_with("PXEClient") || class.starts_with("HTTPClient"))
}

/// 在已通过准入判断后，按 iPXE / 架构选择引导目标。
///
/// # Errors
///
/// 失败时返回对应错误。
fn build_target(
    req: &DhcpRequestView,
    table: &DirectiveTable,
    rules: DhcpRules,
) -> Result<BootOffer, IgnoreReason> {
    let entry = table.find(req.mac).ok_or(IgnoreReason::NotArmed)?;
    if req.is_ipxe() {
        return script_offer(table, entry);
    }
    match req.client_arch() {
        ClientArch::BiosX86 => Ok(tftp_offer(entry, FirmwareName::UndionlyKpxe)),
        ClientArch::UefiX64 => Ok(tftp_offer(
            entry,
            FirmwareName::from_uefi(rules.uefi_firmware),
        )),
        ClientArch::UefiX64Http => http_boot_offer(req, table, entry, rules),
        ClientArch::Unsupported(_) => Err(IgnoreReason::UnsupportedArch),
    }
}

/// iPXE 脚本 URL 要约。
///
/// # Errors
///
/// 失败时返回对应错误。
fn script_offer(table: &DirectiveTable, entry: &BootDirective) -> Result<BootOffer, IgnoreReason> {
    let path = format!("/p/{}/boot.ipxe", entry.token.as_str());
    let url = join_url(&table.boot_base, &path).map_err(|_error| IgnoreReason::NotArmed)?;
    Ok(BootOffer {
        mac: entry.mac,
        session: entry.session.clone(),
        target: BootTarget::Http(url),
        http_client_class: false,
    })
}

/// TFTP 固件要约。
///
/// # Errors
///
/// 失败时返回对应错误。
fn tftp_offer(entry: &BootDirective, file: FirmwareName) -> BootOffer {
    BootOffer {
        mac: entry.mac,
        session: entry.session.clone(),
        target: BootTarget::Tftp(file),
        http_client_class: false,
    }
}

/// UEFI HTTP Boot：要求选项 60 以 `HTTPClient` 开头且规则允许。
///
/// # Errors
///
/// 失败时返回对应错误。
fn http_boot_offer(
    req: &DhcpRequestView,
    table: &DirectiveTable,
    entry: &BootDirective,
    rules: DhcpRules,
) -> Result<BootOffer, IgnoreReason> {
    let vendor = req.vendor_class.as_deref().unwrap_or("");
    if !vendor.starts_with("HTTPClient") {
        return Err(IgnoreReason::UnsupportedArch);
    }
    if !rules.http_boot_enabled {
        return Err(IgnoreReason::HttpBootDisabled);
    }
    let url =
        join_url(&table.boot_base, "/fw/x64/ipxe.efi").map_err(|_error| IgnoreReason::NotArmed)?;
    Ok(BootOffer {
        mac: entry.mac,
        session: entry.session.clone(),
        target: BootTarget::Http(url),
        http_client_class: true,
    })
}

/// 拼接基址与以 `/` 开头的路径。
///
/// # Errors
///
/// 失败时返回对应错误。
fn join_url(base: &HttpUrl, path: &str) -> Result<HttpUrl, ParseError> {
    let trimmed = base.as_str().trim_end_matches('/');
    let joined = format!("{trimmed}{path}");
    HttpUrl::parse(&joined)
}

/// iPXE 入口决策：仅 `Armed` 返回链式加载，其余一律 `exit`。
///
/// # Errors
///
/// 失败时返回对应错误。
#[must_use]
pub fn ipxe_entry(_mac: MacAddr, runtime: &NodeRuntime) -> IpxeEntry {
    match (runtime.state(), runtime.session()) {
        (NodeState::Armed, Some(session)) => IpxeEntry::Chain(session.clone()),
        (state, Some(_) | None) => IpxeEntry::Exit(state),
    }
}

impl crate::versioning::ConfigDocument for DhcpRules {
    const KIND: crate::versioning::ConfigKind = crate::versioning::ConfigKind::DhcpRules;
    const SCHEMA: u32 = 1;

    fn validate(&self) -> Result<(), Vec<crate::versioning::FieldError>> {
        Ok(())
    }

    fn migrate(
        from_schema: u32,
        value: serde_json::Value,
    ) -> Result<Self, crate::versioning::ConfigError> {
        crate::versioning::migrate_current(from_schema, value)
    }
}

#[cfg(test)]
mod tests;
