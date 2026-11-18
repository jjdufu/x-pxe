//! 访问控制模块：sshd drop-in 与 sudoers。

use serde::{Deserialize, Serialize};

use super::system::parsed;
use super::types::{EnvName, Flag, PlainPath, Ranged, SafeFileName};
use crate::provision::UnixGroup;
use crate::value::UnixUser;

/// `PermitRootLogin`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RootLogin {
    /// `no`。
    #[default]
    No,
    /// `prohibit-password`。
    ProhibitPassword,
    /// `yes`。
    Yes,
}

impl RootLogin {
    /// sshd 取值。
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::No => "no",
            Self::ProhibitPassword => "prohibit-password",
            Self::Yes => "yes",
        }
    }
}

/// 模块 9：sshd drop-in。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Sshd {
    /// drop-in 文件名。
    pub dropin_name: SafeFileName,
    /// `PermitRootLogin`。
    pub permit_root_login: RootLogin,
    /// `PasswordAuthentication`。
    pub password_authentication: Flag,
    /// `X11Forwarding`。
    pub x11_forwarding: Flag,
    /// `X11UseLocalhost`。
    pub x11_use_localhost: Flag,
    /// `AllowTcpForwarding`。
    pub allow_tcp_forwarding: Flag,
    /// `AllowAgentForwarding`。
    pub allow_agent_forwarding: Flag,
    /// `IgnoreRhosts`。
    pub ignore_rhosts: Flag,
    /// `GSSAPIAuthentication`。
    pub gssapi_authentication: Flag,
    /// `UseDNS`。
    pub use_dns: Flag,
    /// `Compression`。
    pub compression: Flag,
    /// `TCPKeepAlive`。
    pub tcp_keepalive: Flag,
    /// `Banner`。
    pub banner: Option<PlainPath>,
    /// 主配置同步 `X11Forwarding` 与 `Banner`。
    pub sync_main_config: Flag,
}

impl Default for Sshd {
    fn default() -> Self {
        Self {
            dropin_name: SafeFileName::from_static("jwczjzl.conf"),
            permit_root_login: RootLogin::No,
            password_authentication: Flag::OFF,
            x11_forwarding: Flag::OFF,
            x11_use_localhost: Flag::ON,
            allow_tcp_forwarding: Flag::ON,
            allow_agent_forwarding: Flag::ON,
            ignore_rhosts: Flag::ON,
            gssapi_authentication: Flag::OFF,
            use_dns: Flag::OFF,
            compression: Flag::OFF,
            tcp_keepalive: Flag::ON,
            banner: Some(PlainPath::from_static("/etc/issue")),
            sync_main_config: Flag::ON,
        }
    }
}

/// sudoers `Defaults`。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SudoDefaults {
    /// `env_reset`。
    pub env_reset: Flag,
    /// `env_keep +=`。
    pub env_keep: Vec<EnvName>,
    /// `env_delete =`。
    pub env_delete: Vec<EnvName>,
    /// `passwd_tries`。
    pub passwd_tries: Ranged<1, 10>,
    /// `timestamp_timeout`（分钟）。
    pub timestamp_timeout_min: Ranged<0, 60>,
    /// `secure_path`。
    pub secure_path: Vec<PlainPath>,
    /// `use_pty`。
    pub use_pty: Flag,
    /// `logfile`。
    pub logfile: PlainPath,
}

impl Default for SudoDefaults {
    fn default() -> Self {
        Self {
            env_reset: Flag::ON,
            env_keep: parsed(&["HOME"]),
            env_delete: parsed(&["HTTP_PROXY", "HTTPS_PROXY"]),
            passwd_tries: Ranged::clamped(3),
            timestamp_timeout_min: Ranged::clamped(10),
            secure_path: parsed(&[
                "/usr/local/sbin",
                "/usr/local/bin",
                "/usr/sbin",
                "/usr/bin",
                "/sbin",
                "/bin",
            ]),
            use_pty: Flag::ON,
            logfile: PlainPath::from_static("/var/log/sudo.log"),
        }
    }
}

/// 免密命令别名。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SudoAlias {
    /// 别名（`[A-Z_][A-Z0-9_]*`）。
    pub name: EnvName,
    /// 命令绝对路径。
    pub commands: Vec<PlainPath>,
    /// 适用组（渲染为 `%group`）。
    pub groups: Vec<UnixGroup>,
}

impl Default for SudoAlias {
    fn default() -> Self {
        Self {
            name: EnvName::from_static("SYSADMIN"),
            commands: parsed(&[
                "/usr/sbin/service",
                "/usr/bin/systemctl",
                "/usr/bin/apt-get",
                "/usr/bin/apt",
                "/usr/sbin/reboot",
                "/usr/sbin/shutdown",
            ]),
            groups: parsed(&["sudo"]),
        }
    }
}

/// logrotate 周期。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RotatePeriod {
    /// 每天。
    Daily,
    /// 每周。
    #[default]
    Weekly,
    /// 每月。
    Monthly,
}

impl RotatePeriod {
    /// logrotate 关键字。
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Daily => "daily",
            Self::Weekly => "weekly",
            Self::Monthly => "monthly",
        }
    }
}

/// sudo 日志轮转。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SudoLogrotate {
    /// 周期。
    pub period: RotatePeriod,
    /// 保留份数。
    pub rotate: Ranged<1, 52>,
    /// 压缩。
    pub compress: Flag,
}

impl Default for SudoLogrotate {
    fn default() -> Self {
        Self {
            period: RotatePeriod::Weekly,
            rotate: Ranged::clamped(4),
            compress: Flag::ON,
        }
    }
}

/// 模块 10：sudoers。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Sudoers {
    /// `/etc/sudoers.d/` 下的文件名。
    pub dropin_name: SafeFileName,
    /// `Defaults`。
    pub defaults: SudoDefaults,
    /// 免密命令别名。
    pub nopasswd_alias: SudoAlias,
    /// 拥有完整 sudo（需口令）的用户。
    pub full_sudo_users: Vec<UnixUser>,
    /// 备份 `/etc/sudoers` 到 `/etc/sudoers.ori`。
    pub backup_original: Flag,
    /// `/etc/logrotate.d/sudo`。
    pub logrotate: SudoLogrotate,
}

impl Default for Sudoers {
    fn default() -> Self {
        Self {
            dropin_name: SafeFileName::from_static("jwczjzl"),
            defaults: SudoDefaults::default(),
            nopasswd_alias: SudoAlias::default(),
            full_sudo_users: Vec::new(),
            backup_original: Flag::ON,
            logrotate: SudoLogrotate::default(),
        }
    }
}
