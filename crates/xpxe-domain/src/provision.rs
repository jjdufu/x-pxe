//! 装机 Profile：区域、镜像源、账户、选盘与分区方案。
//!
//! 模块文件名为 `provision` 而非 `profile`，以免与类型 [`Profile`] 触发
//! `module_name_repetitions`（类型名保持不变）。

mod accounts;
mod apt;
mod host;
mod layout;
mod recipe;
mod software;
mod storage;

pub use accounts::{
    CryptHash, CryptSecret, InstallAccounts, PrimaryUser, RootAccount, UnixGroup, hash_fingerprint,
};
pub use apt::{AptComponent, AptRepo, ArmoredKey, ComponentSet, SecuritySource, SuiteSet};
pub use host::{ConsoleSet, ConsoleSpec, HostPort, Mirror, MirrorScheme, NtpServer, UnixAbsPath};
pub use layout::Keymap;
pub use recipe::{FsType, MountPoint, PartitionScheme, PartitionSpec, SizeSpec};
pub use software::{InstallerDebug, TaskName, TaskSet, Upgrade};
pub use storage::{DiskMatch, DiskRule, DiskTransport, Prefer, SizeRange, default_exclude_models};

use serde::{Deserialize, Serialize};

use crate::value::{Locale, SafeText, TimeZone};
use crate::versioning::{
    ConfigDocument, ConfigError, ConfigKind, FieldError, ValidationError, field_error,
    migrate_current,
};

/// 内置预设：与现网 `legacy/pxe-server.sh` 装机参数一致（口令除外）。
const PRESET_NEXUS_LVM: &str = include_str!("../data/presets/nexus-lvm.json");
/// 内置预设：官方 Debian 镜像与安全更新。
const PRESET_OFFICIAL_MIRROR: &str = include_str!("../data/presets/official-mirror.json");

/// 尚未做跨字段校验的 Profile 字段组。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileParts {
    /// 显示名称。
    pub name: SafeText,
    /// 安装器 locale，默认 `en_US.UTF-8`。
    #[serde(default = "default_locale")]
    pub locale: Locale,
    /// 键盘布局，默认 `us`。
    #[serde(default = "default_keymap")]
    pub keymap: Keymap,
    /// 时区，默认 `Asia/Shanghai`。
    #[serde(default = "default_timezone")]
    pub timezone: TimeZone,
    /// 安装期 NTP 服务器，可空。
    #[serde(default)]
    pub ntp: Vec<NtpServer>,
    /// APT 镜像源。
    pub mirror: Mirror,
    /// 安全更新源。
    pub security: SecuritySource,
    /// updates / backports 开关。
    #[serde(default)]
    pub suites: SuiteSet,
    /// APT 组件。
    #[serde(default)]
    pub components: ComponentSet,
    /// 额外 APT 仓库。
    #[serde(default)]
    pub extra_repos: Vec<AptRepo>,
    /// 根账户与主用户。
    pub accounts: InstallAccounts,
    /// tasksel 任务。
    #[serde(default)]
    pub tasks: TaskSet,
    /// 软件升级策略。
    #[serde(default)]
    pub pkgsel_upgrade: Upgrade,
    /// 选盘规则。
    #[serde(default)]
    pub disk: DiskRule,
    /// 分区方案。
    pub partitioning: PartitionScheme,
    /// 内核控制台。
    #[serde(default)]
    pub console: ConsoleSet,
    /// 是否追加 Debian firmware.cpio.gz。
    #[serde(default)]
    pub firmware_cpio: bool,
    /// 安装器调试级别。
    #[serde(default)]
    pub debug: InstallerDebug,
}

/// 已通过跨字段校验的装机 Profile。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "ProfileParts", into = "ProfileParts")]
pub struct Profile {
    /// 字段组。
    parts: ProfileParts,
}

impl Profile {
    /// 由字段组构造并校验。
    ///
    /// # Errors
    ///
    /// 跨字段约束失败时返回全部字段错误。
    pub fn new(parts: ProfileParts) -> Result<Self, Vec<FieldError>> {
        let profile = Self { parts };
        profile.check_constraints()?;
        Ok(profile)
    }

    /// 内置预设名。
    #[must_use]
    pub const fn preset_names() -> &'static [&'static str] {
        &["nexus-lvm", "official-mirror"]
    }

    /// 加载内置预设。
    ///
    /// # Errors
    ///
    /// 名称未知、JSON 无法解码或校验失败时返回错误。
    pub fn load_preset(name: &str) -> Result<Self, ConfigError> {
        load_preset_inner(name)
    }

    /// 跨字段约束。
    ///
    /// # Errors
    ///
    /// 违反账户、分区、选盘或调试约束时返回错误。
    pub fn check_constraints(&self) -> Result<(), Vec<FieldError>> {
        let mut errors = Vec::new();
        self.parts.accounts.collect_errors(&mut errors);
        self.parts.partitioning.collect_errors(&mut errors);
        collect_disk_errors(&self.parts.disk, &mut errors);
        collect_debug_errors(self.parts.debug, &mut errors);
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// `XPXE_OFFLINE=true` 且安全源为官方时给出警告（不阻止校验）。
    #[must_use]
    pub fn offline_warnings(&self, offline: bool) -> Vec<FieldError> {
        official_offline_warning(offline, &self.parts.security)
    }

    /// 字段组引用。
    #[must_use]
    pub const fn parts(&self) -> &ProfileParts {
        &self.parts
    }

    /// 显示名称。
    #[must_use]
    pub const fn name(&self) -> &SafeText {
        &self.parts.name
    }

    /// locale。
    #[must_use]
    pub const fn locale(&self) -> &Locale {
        &self.parts.locale
    }

    /// 键盘布局。
    #[must_use]
    pub const fn keymap(&self) -> &Keymap {
        &self.parts.keymap
    }

    /// 时区。
    #[must_use]
    pub const fn timezone(&self) -> &TimeZone {
        &self.parts.timezone
    }

    /// NTP 服务器。
    #[must_use]
    pub fn ntp(&self) -> &[NtpServer] {
        &self.parts.ntp
    }

    /// 镜像源。
    #[must_use]
    pub const fn mirror(&self) -> &Mirror {
        &self.parts.mirror
    }

    /// 安全更新源。
    #[must_use]
    pub const fn security(&self) -> &SecuritySource {
        &self.parts.security
    }

    /// updates / backports。
    #[must_use]
    pub const fn suites(&self) -> SuiteSet {
        self.parts.suites
    }

    /// APT 组件。
    #[must_use]
    pub const fn components(&self) -> &ComponentSet {
        &self.parts.components
    }

    /// 额外仓库。
    #[must_use]
    pub fn extra_repos(&self) -> &[AptRepo] {
        &self.parts.extra_repos
    }

    /// 账户。
    #[must_use]
    pub const fn accounts(&self) -> &InstallAccounts {
        &self.parts.accounts
    }

    /// 任务集。
    #[must_use]
    pub const fn tasks(&self) -> &TaskSet {
        &self.parts.tasks
    }

    /// 升级策略。
    #[must_use]
    pub const fn pkgsel_upgrade(&self) -> Upgrade {
        self.parts.pkgsel_upgrade
    }

    /// 选盘规则。
    #[must_use]
    pub const fn disk(&self) -> &DiskRule {
        &self.parts.disk
    }

    /// 分区方案。
    #[must_use]
    pub const fn partitioning(&self) -> &PartitionScheme {
        &self.parts.partitioning
    }

    /// 控制台。
    #[must_use]
    pub const fn console(&self) -> &ConsoleSet {
        &self.parts.console
    }

    /// 是否追加 firmware.cpio.gz。
    #[must_use]
    pub const fn firmware_cpio(&self) -> bool {
        self.parts.firmware_cpio
    }

    /// 调试级别。
    #[must_use]
    pub const fn debug(&self) -> InstallerDebug {
        self.parts.debug
    }
}

/// 加载预设的实现，控制 `load_preset` 行数。
///
/// # Errors
///
/// 名称未知、JSON 无法解码或校验失败时返回错误。
fn load_preset_inner(name: &str) -> Result<Profile, ConfigError> {
    let raw = match name {
        "nexus-lvm" => PRESET_NEXUS_LVM,
        "official-mirror" => PRESET_OFFICIAL_MIRROR,
        _ => return Err(ConfigError::Json(format!("未知预设 {name}"))),
    };
    let parts: ProfileParts = serde_json::from_str(raw)?;
    Profile::new(parts)
        .map_err(|errors| ConfigError::Invalid(Box::new(ValidationError::from_fields(errors))))
}

/// 官方安全源在离线模式下的警告。
fn official_offline_warning(offline: bool, security: &SecuritySource) -> Vec<FieldError> {
    if offline && matches!(security, SecuritySource::Official) {
        vec![field_error(
            "/security",
            "official_unavailable_offline",
            "离线模式下官方安全更新源不可达，请改用镜像或禁用",
        )]
    } else {
        Vec::new()
    }
}

/// 选盘规则错误。
fn collect_disk_errors(disk: &DiskRule, errors: &mut Vec<FieldError>) {
    if let Err(error) = disk.check() {
        errors.push(field_error("/disk", error.code(), "选盘规则不合法"));
    }
}

/// 调试级别错误。
fn collect_debug_errors(debug: InstallerDebug, errors: &mut Vec<FieldError>) {
    if !debug.is_valid() {
        errors.push(field_error(
            "/debug/level",
            "debug_level",
            "DEBCONF_DEBUG 级别必须为 1–5",
        ));
    }
}

/// 默认 locale。
fn default_locale() -> Locale {
    Locale::from_verified("en_US.UTF-8")
}

/// 默认键盘。
fn default_keymap() -> Keymap {
    Keymap::from_verified("us")
}

/// 默认时区。
fn default_timezone() -> TimeZone {
    TimeZone::from_verified("Asia/Shanghai")
}

impl TryFrom<ProfileParts> for Profile {
    type Error = ValidationError;

    fn try_from(parts: ProfileParts) -> Result<Self, Self::Error> {
        Self::new(parts).map_err(ValidationError::from_fields)
    }
}

impl From<Profile> for ProfileParts {
    fn from(value: Profile) -> Self {
        value.parts
    }
}

impl ConfigDocument for Profile {
    const KIND: ConfigKind = ConfigKind::Profile;
    const SCHEMA: u32 = 1;

    fn validate(&self) -> Result<(), Vec<FieldError>> {
        self.check_constraints()
    }

    fn migrate(from_schema: u32, value: serde_json::Value) -> Result<Self, ConfigError> {
        migrate_current(from_schema, value)
    }
}

#[cfg(test)]
mod tests;
