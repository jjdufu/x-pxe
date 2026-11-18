//! 系统基础模块：APT 源、软件包、用户与公钥、时区、时间同步、locale、systemd 超时、ssh generator 修复。

use serde::{Deserialize, Serialize};

use super::catalog::PackageGroups;
use super::types::{Flag, Ranged, UnitName};
use crate::provision::{AptComponent, CryptSecret, NtpServer, UnixGroup};
use crate::value::{Locale, PackageName, SafeText, SshPublicKey, TimeZone, UnixUser};

/// 解析一组内置字面量，丢弃非法项（内置项的合法性由测试保证）。
pub(crate) fn parsed<T: TryFrom<String>>(items: &[&str]) -> Vec<T> {
    items
        .iter()
        .filter_map(|raw| T::try_from((*raw).to_owned()).ok())
        .collect()
}

/// 官方源的套件。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OfficialSuite {
    /// `trixie`。
    #[serde(rename = "trixie")]
    Trixie,
    /// `trixie-updates`。
    #[serde(rename = "trixie-updates")]
    TrixieUpdates,
    /// `trixie-backports`。
    #[serde(rename = "trixie-backports")]
    TrixieBackports,
}

impl OfficialSuite {
    /// 套件名。
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Trixie => "trixie",
            Self::TrixieUpdates => "trixie-updates",
            Self::TrixieBackports => "trixie-backports",
        }
    }
}

/// 没有任何网络源时写入的兜底源。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum AptFallback {
    /// Debian 官方源（deb.debian.org，deb822）。
    Official {
        /// 套件。
        suites: Vec<OfficialSuite>,
        /// 是否写安全更新源。
        security: Flag,
        /// 组件。
        components: Vec<AptComponent>,
    },
    /// 使用 Profile 的镜像源。
    ProfileMirror,
    /// 不写兜底源。
    #[serde(rename = "none")]
    Off,
}

impl Default for AptFallback {
    fn default() -> Self {
        Self::Official {
            suites: vec![
                OfficialSuite::Trixie,
                OfficialSuite::TrixieUpdates,
                OfficialSuite::TrixieBackports,
            ],
            security: Flag::ON,
            components: vec![
                AptComponent::Main,
                AptComponent::Contrib,
                AptComponent::NonFree,
                AptComponent::NonFreeFirmware,
            ],
        }
    }
}

/// 模块 1：APT 源整理。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AptSources {
    /// 注释 `deb cdrom:` 行并禁用 cdrom 的 `.sources`。
    pub disable_cdrom: Flag,
    /// 恢复被注释的 http(s) 行。
    pub uncomment_http: Flag,
    /// 无网络源时的兜底。
    pub fallback: AptFallback,
    /// `apt-get update` 前先 `autoclean`。
    pub autoclean_before_update: Flag,
}

impl Default for AptSources {
    fn default() -> Self {
        Self {
            disable_cdrom: Flag::ON,
            uncomment_http: Flag::ON,
            fallback: AptFallback::default(),
            autoclean_before_update: Flag::ON,
        }
    }
}

/// 模块 2：软件包。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Packages {
    /// 8 个分组开关。
    pub groups: PackageGroups,
    /// 额外包。
    pub extra: Vec<PackageName>,
    /// 从分组中排除的包。
    pub exclude: Vec<PackageName>,
    /// 安装推荐包。
    pub install_recommends: Flag,
    /// 安装 `zram-tools`。
    pub zram_tools: Flag,
}

impl Default for Packages {
    fn default() -> Self {
        Self {
            groups: PackageGroups::default(),
            extra: Vec::new(),
            exclude: Vec::new(),
            install_recommends: Flag::ON,
            zram_tools: Flag::OFF,
        }
    }
}

/// `useradd` 默认 shell。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum DefaultShell {
    /// `/bin/bash`。
    #[default]
    #[serde(rename = "/bin/bash")]
    Bash,
    /// `/bin/sh`。
    #[serde(rename = "/bin/sh")]
    Sh,
}

impl DefaultShell {
    /// 路径。
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Bash => "/bin/bash",
            Self::Sh => "/bin/sh",
        }
    }
}

/// 用户登录 shell。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum UserShell {
    /// `/bin/bash`。
    #[default]
    #[serde(rename = "/bin/bash")]
    Bash,
    /// `/bin/sh`。
    #[serde(rename = "/bin/sh")]
    Sh,
    /// `/usr/sbin/nologin`（服务账户）。
    #[serde(rename = "/usr/sbin/nologin")]
    Nologin,
}

impl UserShell {
    /// 路径。
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Bash => "/bin/bash",
            Self::Sh => "/bin/sh",
            Self::Nologin => "/usr/sbin/nologin",
        }
    }
}

/// 用户口令。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum UserPassword {
    /// 锁定口令（只能用公钥登录）。
    #[default]
    Locked,
    /// 给定 crypt 哈希。
    Hash {
        /// 哈希。
        hash: CryptSecret,
    },
}

/// 默认附加组 `[sudo]`。
fn default_groups() -> Vec<UnixGroup> {
    parsed(&["sudo"])
}

/// 默认开。
const fn flag_on() -> Flag {
    Flag::ON
}

/// 一个受管用户。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedUser {
    /// 登录名（不能是 root）。
    pub name: UnixUser,
    /// 登录 shell。
    #[serde(default)]
    pub shell: UserShell,
    /// 口令。
    #[serde(default)]
    pub password: UserPassword,
    /// 附加组。
    #[serde(default = "default_groups")]
    pub groups: Vec<UnixGroup>,
    /// 公钥。
    #[serde(default)]
    pub authorized_keys: Vec<SshPublicKey>,
    /// 复制 root 的公钥。
    #[serde(default = "flag_on")]
    pub copy_root_keys: Flag,
    /// `loginctl enable-linger`。
    #[serde(default = "flag_on")]
    pub linger: Flag,
}

impl ManagedUser {
    /// 是否属于 `sudo` 组。
    #[must_use]
    pub fn has_sudo(&self) -> bool {
        self.groups.iter().any(|group| group.as_str() == "sudo")
    }
}

/// 模块 3：用户与公钥。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct UsersKeys {
    /// `/etc/default/useradd` 的 `SHELL`。
    pub useradd_default_shell: DefaultShell,
    /// root 的公钥。
    pub root_authorized_keys: Vec<SshPublicKey>,
    /// 受管用户。
    pub users: Vec<ManagedUser>,
}

/// 模块 4：时区。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Timezone {
    /// 时区。
    pub zone: TimeZone,
    /// 硬件时钟用 UTC。
    pub rtc_utc: Flag,
}

impl Default for Timezone {
    fn default() -> Self {
        Self {
            zone: TimeZone::from_verified("Asia/Shanghai"),
            rtc_utc: Flag::ON,
        }
    }
}

/// chrony 的一个 pool。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChronyPool {
    /// 主机。
    pub host: NtpServer,
    /// `iburst`。
    #[serde(default = "flag_on")]
    pub iburst: Flag,
    /// `maxsources`。
    #[serde(default = "one_source")]
    pub maxsources: Ranged<1, 8>,
}

/// 默认 `maxsources 1`。
const fn one_source() -> Ranged<1, 8> {
    Ranged::clamped(1)
}

/// 时间同步方式。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum TimeSyncMode {
    /// systemd-timesyncd。
    Timesyncd {
        /// NTP 服务器。
        servers: Vec<NtpServer>,
        /// 确保已安装 `systemd-timesyncd`。
        #[serde(default = "flag_on")]
        ensure_installed: Flag,
    },
    /// chrony。
    Chrony {
        /// pool 列表。
        pools: Vec<ChronyPool>,
    },
}

impl Default for TimeSyncMode {
    fn default() -> Self {
        Self::Timesyncd {
            servers: parsed(&["ntp.aliyun.com"]),
            ensure_installed: Flag::ON,
        }
    }
}

/// 模块 5：时间同步。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct TimeSync {
    /// 方式。
    pub mode: TimeSyncMode,
}

/// 模块 6：locale。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LocaleSettings {
    /// `LANG`。
    pub lang: Locale,
    /// `LC_ALL`。
    pub lc_all: Option<Locale>,
    /// `LANGUAGE`（字符集 `[A-Za-z0-9_.:@-]`）。
    pub language: SafeText,
    /// 需要生成的 locale。
    pub generate: Vec<Locale>,
}

impl Default for LocaleSettings {
    fn default() -> Self {
        Self {
            lang: Locale::from_verified("en_US.UTF-8"),
            lc_all: Some(Locale::from_verified("en_US.UTF-8")),
            language: SafeText::from_verified("en_US.UTF-8"),
            generate: vec![
                Locale::from_verified("zh_CN.UTF-8"),
                Locale::from_verified("en_US.UTF-8"),
            ],
        }
    }
}

/// 模块 7：systemd 启停超时。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SystemdManager {
    /// `DefaultTimeoutStartSec`。
    pub default_timeout_start_s: Ranged<5, 600>,
    /// `DefaultTimeoutStopSec`。
    pub default_timeout_stop_s: Ranged<5, 600>,
}

impl Default for SystemdManager {
    fn default() -> Self {
        Self {
            default_timeout_start_s: Ranged::clamped(30),
            default_timeout_stop_s: Ranged::clamped(30),
        }
    }
}

/// 模块 8：ssh generator 刷屏修复。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SshGeneratorFix {
    /// 用 `/dev/null` 覆盖 `systemd-ssh-generator`。
    pub mask_generator: Flag,
    /// 需要 mask 的 socket 单元。
    pub mask_sockets: Vec<UnitName>,
}

impl Default for SshGeneratorFix {
    fn default() -> Self {
        Self {
            mask_generator: Flag::ON,
            mask_sockets: vec![
                UnitName::from_static("sshd-unix-local.socket"),
                UnitName::from_static("sshd-vsock.socket"),
            ],
        }
    }
}
