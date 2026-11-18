//! 初始化模块：把 `legacy/init-d13.sh` 拆成 20 个有类型、有范围、有默认值的模块。
//!
//! 默认值即 `k8s-node` 预设中与具体用户无关的部分；用户相关字段（受管用户、公钥、
//! `vimrc.targets`、`sudoers.full_sudo_users`）由预设 JSON 给出。

mod access;
mod catalog;
mod kernel;
mod shell;
mod system;
mod types;
mod wire;

pub use access::{RootLogin, RotatePeriod, Sshd, SudoAlias, SudoDefaults, SudoLogrotate, Sudoers};
pub use catalog::{PackageCatalog, PackageGroup, PackageGroups};
pub use kernel::{
    BufSize, BufTriple, BufferParams, Congestion, ConnTableParams, Count, ForwardParams,
    KeepaliveParams, LimitValue, Limits, MemoryParams, PortRange, QueueParams, SecurityParams,
    Sysctl, SysctlExtra, SysctlParams, SysctlPreset, TcpParams, WatchParams,
};
pub use shell::{
    Cleanup, CustomSnippet, Fstrim, HistoryAudit, Motd, MotdHeader, Services, ShellProfile,
    Snippet, SshAgentProfile, Vimrc,
};
pub use system::{
    AptFallback, AptSources, ChronyPool, DefaultShell, LocaleSettings, ManagedUser, OfficialSuite,
    Packages, SshGeneratorFix, SystemdManager, TimeSync, TimeSyncMode, Timezone, UserPassword,
    UserShell, UsersKeys,
};
pub use types::{
    EnvName, FileMode, Flag, PlainPath, Ranged, RelPath, SafeFileName, ShellText, SysctlKey,
    SysctlValue, UnitName,
};
pub use wire::{PLAN_FILE_SCHEMA, PlanEntry, PlanFile, PlanFinalize};

use serde::{Deserialize, Serialize};

use crate::provision::PrimaryUser;
use crate::value::{PackageName, ParseError, SafeText, UnixUser};
use crate::versioning::{
    ConfigDocument, ConfigError, ConfigKind, FieldError, ValidationError, field_error,
    migrate_current,
};

/// 内置预设 `k8s-node`。
const PRESET_K8S_NODE: &str = include_str!("../data/init-presets/k8s-node.json");
/// 内置预设 `minimal`。
const PRESET_MINIMAL: &str = include_str!("../data/init-presets/minimal.json");

/// 模块 id（顺序即执行顺序）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModuleId {
    /// APT 源整理。
    AptSources,
    /// 软件包。
    Packages,
    /// 用户与公钥。
    UsersKeys,
    /// 时区。
    Timezone,
    /// 时间同步。
    TimeSync,
    /// locale。
    Locale,
    /// systemd 启停超时。
    SystemdManager,
    /// ssh generator 修复。
    SshGeneratorFix,
    /// sshd。
    Sshd,
    /// sudoers。
    Sudoers,
    /// shell profile。
    ShellProfile,
    /// vimrc。
    Vimrc,
    /// 服务。
    Services,
    /// MOTD。
    Motd,
    /// sysctl。
    Sysctl,
    /// limits。
    Limits,
    /// 命令历史审计。
    HistoryAudit,
    /// fstrim。
    Fstrim,
    /// 自定义片段。
    CustomSnippet,
    /// 清理。
    Cleanup,
}

impl ModuleId {
    /// 固定执行顺序。
    pub const ALL: [Self; 20] = [
        Self::AptSources,
        Self::Packages,
        Self::UsersKeys,
        Self::Timezone,
        Self::TimeSync,
        Self::Locale,
        Self::SystemdManager,
        Self::SshGeneratorFix,
        Self::Sshd,
        Self::Sudoers,
        Self::ShellProfile,
        Self::Vimrc,
        Self::Services,
        Self::Motd,
        Self::Sysctl,
        Self::Limits,
        Self::HistoryAudit,
        Self::Fstrim,
        Self::CustomSnippet,
        Self::Cleanup,
    ];

    /// 蛇形 id。
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AptSources => "apt_sources",
            Self::Packages => "packages",
            Self::UsersKeys => "users_keys",
            Self::Timezone => "timezone",
            Self::TimeSync => "time_sync",
            Self::Locale => "locale",
            Self::SystemdManager => "systemd_manager",
            Self::SshGeneratorFix => "ssh_generator_fix",
            Self::Sshd => "sshd",
            Self::Sudoers => "sudoers",
            Self::ShellProfile => "shell_profile",
            Self::Vimrc => "vimrc",
            Self::Services => "services",
            Self::Motd => "motd",
            Self::Sysctl => "sysctl",
            Self::Limits => "limits",
            Self::HistoryAudit => "history_audit",
            Self::Fstrim => "fstrim",
            Self::CustomSnippet => "custom_snippet",
            Self::Cleanup => "cleanup",
        }
    }

    /// 中文标题。
    #[must_use]
    pub const fn title(self) -> &'static str {
        match self {
            Self::AptSources => "APT 源整理",
            Self::Packages => "软件包",
            Self::UsersKeys => "用户与公钥",
            Self::Timezone => "时区",
            Self::TimeSync => "时间同步",
            Self::Locale => "语言环境",
            Self::SystemdManager => "systemd 启停超时",
            Self::SshGeneratorFix => "ssh generator 刷屏修复",
            Self::Sshd => "SSH 服务配置",
            Self::Sudoers => "sudoers",
            Self::ShellProfile => "Shell 环境",
            Self::Vimrc => "vimrc",
            Self::Services => "系统服务",
            Self::Motd => "登录提示",
            Self::Sysctl => "内核参数",
            Self::Limits => "资源限制",
            Self::HistoryAudit => "命令历史审计",
            Self::Fstrim => "fstrim",
            Self::CustomSnippet => "自定义片段",
            Self::Cleanup => "清理",
        }
    }

    /// 默认超时（秒）。
    #[must_use]
    pub const fn default_timeout_s(self) -> u64 {
        match self {
            Self::Packages => 3600,
            Self::AptSources | Self::CustomSnippet | Self::Cleanup => 900,
            Self::UsersKeys
            | Self::Timezone
            | Self::TimeSync
            | Self::Locale
            | Self::SystemdManager
            | Self::SshGeneratorFix
            | Self::Sshd
            | Self::Sudoers
            | Self::ShellProfile
            | Self::Vimrc
            | Self::Services
            | Self::Motd
            | Self::Sysctl
            | Self::Limits
            | Self::HistoryAudit
            | Self::Fstrim => 300,
        }
    }
}

/// 一个模块的启用状态、执行参数与设置。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Slot<T> {
    /// 是否执行。
    pub enabled: Flag,
    /// 超时（秒），缺省用模块默认值。
    pub timeout_s: Option<Ranged<10, 86_400>>,
    /// 失败后继续执行后续模块。
    pub continue_on_error: Flag,
    /// 模块设置。
    pub settings: T,
}

impl<T: Default> Default for Slot<T> {
    fn default() -> Self {
        Self {
            enabled: Flag::ON,
            timeout_s: None,
            continue_on_error: Flag::OFF,
            settings: T::default(),
        }
    }
}

impl<T> Slot<T> {
    /// 只读执行参数视图。
    const fn view(&self, id: ModuleId) -> SlotView {
        SlotView {
            enabled: self.enabled,
            timeout_s: match self.timeout_s {
                Some(value) => value.get(),
                None => id.default_timeout_s(),
            },
            continue_on_error: self.continue_on_error,
        }
    }
}

/// 模块执行参数。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlotView {
    /// 是否执行。
    pub enabled: Flag,
    /// 生效的超时（秒）。
    pub timeout_s: u64,
    /// 失败后继续。
    pub continue_on_error: Flag,
}

/// 20 个模块。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ModuleSet {
    /// 1 APT 源整理。
    pub apt_sources: Slot<AptSources>,
    /// 2 软件包。
    pub packages: Slot<Packages>,
    /// 3 用户与公钥。
    pub users_keys: Slot<UsersKeys>,
    /// 4 时区。
    pub timezone: Slot<Timezone>,
    /// 5 时间同步。
    pub time_sync: Slot<TimeSync>,
    /// 6 locale。
    pub locale: Slot<LocaleSettings>,
    /// 7 systemd 启停超时。
    pub systemd_manager: Slot<SystemdManager>,
    /// 8 ssh generator 修复。
    pub ssh_generator_fix: Slot<SshGeneratorFix>,
    /// 9 sshd。
    pub sshd: Slot<Sshd>,
    /// 10 sudoers。
    pub sudoers: Slot<Sudoers>,
    /// 11 shell profile。
    pub shell_profile: Slot<ShellProfile>,
    /// 12 vimrc。
    pub vimrc: Slot<Vimrc>,
    /// 13 服务。
    pub services: Slot<Services>,
    /// 14 MOTD。
    pub motd: Slot<Motd>,
    /// 15 sysctl。
    pub sysctl: Slot<Sysctl>,
    /// 16 limits。
    pub limits: Slot<Limits>,
    /// 17 命令历史审计。
    pub history_audit: Slot<HistoryAudit>,
    /// 18 fstrim。
    pub fstrim: Slot<Fstrim>,
    /// 19 自定义片段（默认关闭）。
    pub custom_snippet: Slot<CustomSnippet>,
    /// 20 清理。
    pub cleanup: Slot<Cleanup>,
}

impl Default for ModuleSet {
    fn default() -> Self {
        Self {
            apt_sources: Slot::default(),
            packages: Slot::default(),
            users_keys: Slot::default(),
            timezone: Slot::default(),
            time_sync: Slot::default(),
            locale: Slot::default(),
            systemd_manager: Slot::default(),
            ssh_generator_fix: Slot::default(),
            sshd: Slot::default(),
            sudoers: Slot::default(),
            shell_profile: Slot::default(),
            vimrc: Slot::default(),
            services: Slot::default(),
            motd: Slot::default(),
            sysctl: Slot::default(),
            limits: Slot::default(),
            history_audit: Slot::default(),
            fstrim: Slot::default(),
            custom_snippet: Slot {
                enabled: Flag::OFF,
                ..Slot::default()
            },
            cleanup: Slot::default(),
        }
    }
}

impl ModuleSet {
    /// 某模块的执行参数。
    #[must_use]
    pub const fn view(&self, id: ModuleId) -> SlotView {
        match id {
            ModuleId::AptSources => self.apt_sources.view(id),
            ModuleId::Packages => self.packages.view(id),
            ModuleId::UsersKeys => self.users_keys.view(id),
            ModuleId::Timezone => self.timezone.view(id),
            ModuleId::TimeSync => self.time_sync.view(id),
            ModuleId::Locale => self.locale.view(id),
            ModuleId::SystemdManager => self.systemd_manager.view(id),
            ModuleId::SshGeneratorFix => self.ssh_generator_fix.view(id),
            ModuleId::Sshd => self.sshd.view(id),
            ModuleId::Sudoers => self.sudoers.view(id),
            ModuleId::ShellProfile => self.shell_profile.view(id),
            ModuleId::Vimrc => self.vimrc.view(id),
            ModuleId::Services => self.services.view(id),
            ModuleId::Motd => self.motd.view(id),
            ModuleId::Sysctl => self.sysctl.view(id),
            ModuleId::Limits => self.limits.view(id),
            ModuleId::HistoryAudit => self.history_audit.view(id),
            ModuleId::Fstrim => self.fstrim.view(id),
            ModuleId::CustomSnippet => self.custom_snippet.view(id),
            ModuleId::Cleanup => self.cleanup.view(id),
        }
    }
}

/// 计划级收尾。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Finalize {
    /// 成功后重启（对应 legacy 末尾的 `reboot`）。
    pub reboot: Flag,
}

impl Default for Finalize {
    fn default() -> Self {
        Self { reboot: Flag::ON }
    }
}

/// 初始化计划（配置文档 `init_plan`）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetPlan {
    /// 显示名称。
    pub name: SafeText,
    /// 模块。
    #[serde(default)]
    pub modules: ModuleSet,
    /// 收尾。
    #[serde(default)]
    pub finalize: Finalize,
}

impl TargetPlan {
    /// 内置预设名。
    pub const PRESETS: [&'static str; 2] = ["k8s-node", "minimal"];

    /// 加载内置预设并校验。
    ///
    /// # Errors
    ///
    /// 未知预设、JSON 损坏或校验失败时返回错误。
    pub fn load_preset(name: &str) -> Result<Self, ConfigError> {
        let raw = match name {
            "k8s-node" => PRESET_K8S_NODE,
            "minimal" => PRESET_MINIMAL,
            _ => return Err(ConfigError::Json(format!("未知初始化预设 {name}"))),
        };
        let plan: Self = serde_json::from_str(raw)?;
        plan.check_with(None).map_err(|errors| {
            ConfigError::Invalid(Box::new(ValidationError::from_fields(errors)))
        })?;
        Ok(plan)
    }

    /// 跨模块校验；`primary` 为 Profile 的主用户（若有）。
    ///
    /// # Errors
    ///
    /// 返回全部违反项。
    pub fn check_with(&self, primary: Option<&PrimaryUser>) -> Result<(), Vec<FieldError>> {
        let mut errors = Vec::new();
        check_login_path(&self.modules, &mut errors);
        check_user_refs(&self.modules, primary, &mut errors);
        check_users(&self.modules.users_keys.settings, &mut errors);
        check_locale(&self.modules.locale.settings, &mut errors);
        check_sysctl(&self.modules.sysctl.settings.params, &mut errors);
        check_packages(&self.modules.packages.settings, &mut errors);
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// 不阻止保存的警告。
    #[must_use]
    pub fn warnings(&self) -> Vec<FieldError> {
        let mut out = Vec::new();
        let modules = &self.modules;
        let masks_nft = modules
            .services
            .settings
            .mask
            .iter()
            .any(|unit| unit.as_str() == "nftables.service");
        if modules.services.enabled.is_on()
            && masks_nft
            && modules.packages.enabled.is_on()
            && modules.packages.settings.groups.security.is_on()
        {
            out.push(field_error(
                "/modules/services/settings/mask",
                "fail2ban_without_firewall",
                "nftables 被 mask 而安全分组会安装 fail2ban：fail2ban 将没有防火墙后端",
            ));
        }
        if modules.sysctl.enabled.is_on()
            && modules.sysctl.settings.params.forward.disable_ipv6.is_on()
        {
            out.push(field_error(
                "/modules/sysctl/settings/params/forward/disable_ipv6",
                "confirm_no_dual_stack",
                "禁用 IPv6 前请确认没有双栈依赖",
            ));
        }
        out
    }
}

impl ConfigDocument for TargetPlan {
    const KIND: ConfigKind = ConfigKind::InitPlan;
    const SCHEMA: u32 = 1;

    fn validate(&self) -> Result<(), Vec<FieldError>> {
        self.check_with(None)
    }

    fn migrate(from_schema: u32, value: serde_json::Value) -> Result<Self, ConfigError> {
        migrate_current(from_schema, value)
    }
}

/// 禁 root 且禁口令登录时，至少一个非 root 用户同时具备 `sudo` 组与公钥。
fn check_login_path(modules: &ModuleSet, errors: &mut Vec<FieldError>) {
    let sshd = &modules.sshd.settings;
    if !modules.sshd.enabled.is_on()
        || sshd.permit_root_login != RootLogin::No
        || sshd.password_authentication.is_on()
    {
        return;
    }
    let users = &modules.users_keys.settings;
    let root_keys = !users.root_authorized_keys.is_empty();
    let reachable = modules.users_keys.enabled.is_on()
        && users.users.iter().any(|user| {
            user.has_sudo()
                && user.shell != UserShell::Nologin
                && (!user.authorized_keys.is_empty() || (user.copy_root_keys.is_on() && root_keys))
        });
    if !reachable {
        errors.push(field_error(
            "/modules/users_keys/settings/users",
            "no_login_path",
            "禁止 root 登录且禁用口令认证时，至少一个非 root 用户必须同时属于 sudo 组并带 SSH 公钥",
        ));
    }
}

/// `vimrc.targets` 与 `sudoers.full_sudo_users` 必须是已知用户。
fn check_user_refs(
    modules: &ModuleSet,
    primary: Option<&PrimaryUser>,
    errors: &mut Vec<FieldError>,
) {
    let known = |name: &UnixUser| {
        modules
            .users_keys
            .settings
            .users
            .iter()
            .any(|user| &user.name == name)
            || primary.is_some_and(|user| &user.name == name)
    };
    for (index, name) in modules.vimrc.settings.targets.iter().enumerate() {
        if name.as_str() != "root" && !known(name) {
            errors.push(field_error(
                &format!("/modules/vimrc/settings/targets/{index}"),
                "unknown_user",
                "vimrc 目标用户不在受管用户或主用户中",
            ));
        }
    }
    for (index, name) in modules.sudoers.settings.full_sudo_users.iter().enumerate() {
        if !known(name) {
            errors.push(field_error(
                &format!("/modules/sudoers/settings/full_sudo_users/{index}"),
                "unknown_user",
                "sudo 用户不在受管用户或主用户中",
            ));
        }
    }
}

/// 受管用户不能是 root，也不能重名。
fn check_users(users: &UsersKeys, errors: &mut Vec<FieldError>) {
    for (index, user) in users.users.iter().enumerate() {
        let path = format!("/modules/users_keys/settings/users/{index}/name");
        if user.name.as_str() == "root" {
            errors.push(field_error(
                &path,
                "root_not_managed",
                "root 由 root_authorized_keys 管理",
            ));
        }
        let first = users.users.iter().position(|other| other.name == user.name);
        if first != Some(index) {
            errors.push(field_error(&path, "duplicate_user", "用户重复"));
        }
    }
}

/// `LANGUAGE` 只允许 locale 字符集与 `:`。
fn check_locale(locale: &LocaleSettings, errors: &mut Vec<FieldError>) {
    let ok = locale
        .language
        .as_str()
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || b"_.:@-".contains(&byte));
    if !ok {
        errors.push(field_error(
            "/modules/locale/settings/language",
            "bad_language",
            "LANGUAGE 只允许字母、数字与 _ . : @ -",
        ));
    }
}

/// sysctl 参数之间的大小关系。
fn check_sysctl(params: &SysctlParams, errors: &mut Vec<FieldError>) {
    let base = "/modules/sysctl/settings/params";
    let bufs = &params.buffers;
    let checks = [
        (
            bufs.tcp_rmem.is_ordered(),
            "buffers/tcp_rmem",
            "tcp_rmem 须满足 min ≤ default ≤ max",
        ),
        (
            bufs.tcp_wmem.is_ordered(),
            "buffers/tcp_wmem",
            "tcp_wmem 须满足 min ≤ default ≤ max",
        ),
        (
            bufs.rmem_default <= bufs.rmem_max,
            "buffers/rmem_default",
            "rmem_default 不能大于 rmem_max",
        ),
        (
            bufs.wmem_default <= bufs.wmem_max,
            "buffers/wmem_default",
            "wmem_default 不能大于 wmem_max",
        ),
        (
            params.memory.dirty_background_ratio.get() < params.memory.dirty_ratio.get(),
            "memory/dirty_background_ratio",
            "dirty_background_ratio 必须小于 dirty_ratio",
        ),
        (
            params.conn_table.ip_local_port_range.low < params.conn_table.ip_local_port_range.high,
            "conn_table/ip_local_port_range",
            "端口范围下界必须小于上界",
        ),
    ];
    for (ok, path, message) in checks {
        if !ok {
            errors.push(field_error(
                &format!("{base}/{path}"),
                "out_of_order",
                message,
            ));
        }
    }
}

/// 额外包与排除包不能重叠。
fn check_packages(packages: &Packages, errors: &mut Vec<FieldError>) {
    let overlap = packages
        .extra
        .iter()
        .any(|name| packages.exclude.contains(name));
    if overlap {
        errors.push(field_error(
            "/modules/packages/settings/exclude",
            "extra_excluded",
            "同一个包不能既在 extra 又在 exclude",
        ));
    }
}

impl Packages {
    /// 解析出最终安装清单：开启分组的并集 + extra − exclude（+ zram-tools）。
    ///
    /// # Errors
    ///
    /// 内置分组数据损坏时返回错误。
    pub fn resolve(&self, catalog: &PackageCatalog) -> Result<Vec<PackageName>, ParseError> {
        let mut out: Vec<PackageName> = Vec::new();
        let groups = PackageGroup::ALL
            .into_iter()
            .filter(|group| self.groups.is_enabled(*group));
        for group in groups {
            out.extend(catalog.group(group).iter().cloned());
        }
        out.extend(self.extra.iter().cloned());
        if self.zram_tools.is_on() {
            out.push(PackageName::parse("zram-tools")?);
        }
        let mut seen: Vec<PackageName> = Vec::new();
        for name in out {
            if !self.exclude.contains(&name) && !seen.contains(&name) {
                seen.push(name);
            }
        }
        Ok(seen)
    }
}

#[cfg(test)]
mod tests;
