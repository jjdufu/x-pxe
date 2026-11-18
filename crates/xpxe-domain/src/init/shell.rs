//! 交互与运维模块：profile.d、vimrc、服务、MOTD、历史审计、fstrim、自定义片段、清理。

use serde::{Deserialize, Serialize};

use super::system::parsed;
use super::types::{FileMode, Flag, PlainPath, Ranged, RelPath, SafeFileName, ShellText, UnitName};
use crate::value::{SafeText, UnixUser};

/// 登录时的 ssh-agent。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SshAgentProfile {
    /// 启用。
    pub enabled: Flag,
    /// 家目录内的私钥路径。
    pub key: RelPath,
}

impl Default for SshAgentProfile {
    fn default() -> Self {
        Self {
            enabled: Flag::ON,
            key: RelPath::from_static(".ssh/id_ed25519"),
        }
    }
}

/// 模块 11：shell profile。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ShellProfile {
    /// ls/grep/fgrep/egrep 着色。
    pub color_aliases: Flag,
    /// ll/la/l。
    pub short_aliases: Flag,
    /// `unset MAILCHECK`。
    pub unset_mailcheck: Flag,
    /// 追加到 PATH 的目录。
    pub path_append: Vec<PlainPath>,
    /// ssh-agent。
    pub ssh_agent: SshAgentProfile,
}

impl Default for ShellProfile {
    fn default() -> Self {
        Self {
            color_aliases: Flag::ON,
            short_aliases: Flag::ON,
            unset_mailcheck: Flag::ON,
            path_append: parsed(&["/usr/local/sbin"]),
            ssh_agent: SshAgentProfile::default(),
        }
    }
}

/// 模块 12：vimrc（内容为内置模板 `vimrc.default`）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Vimrc {
    /// 写入 `~/.vimrc` 的用户。
    pub targets: Vec<UnixUser>,
}

impl Default for Vimrc {
    fn default() -> Self {
        Self {
            targets: UnixUser::parse("root").ok().into_iter().collect(),
        }
    }
}

/// 模块 13：服务。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Services {
    /// `disable --now`。
    pub disable_now: Vec<UnitName>,
    /// `unmask` 后 `enable --now`。
    pub enable_now: Vec<UnitName>,
    /// `mask`。
    pub mask: Vec<UnitName>,
    /// `update-alternatives --set editor`。
    pub default_editor: Option<PlainPath>,
}

impl Default for Services {
    fn default() -> Self {
        Self {
            disable_now: parsed(&["apt-daily.timer", "apt-daily-upgrade.timer"]),
            enable_now: parsed(&["ssh"]),
            mask: parsed(&[
                "cryptdisks-early.service",
                "cryptdisks.service",
                "x11-common.service",
                "nftables.service",
            ]),
            default_editor: Some(PlainPath::from_static("/usr/bin/vim.basic")),
        }
    }
}

/// 登录头部脚本 `00-custom-header`。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MotdHeader {
    /// 启用。
    pub enabled: Flag,
    /// 磁盘条宽度。
    pub bar_width: Ranged<20, 120>,
    /// 告警阈值（百分比）。
    pub warn_percent: Ranged<50, 99>,
    /// `df -x` 排除的文件系统。
    pub exclude_fs: Vec<SafeFileName>,
}

impl Default for MotdHeader {
    fn default() -> Self {
        Self {
            enabled: Flag::ON,
            bar_width: Ranged::clamped(60),
            warn_percent: Ranged::clamped(90),
            exclude_fs: parsed(&["zfs", "squashfs", "tmpfs", "devtmpfs", "overlay"]),
        }
    }
}

/// 模块 14：MOTD 与 Banner。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Motd {
    /// 清空 `/etc/issue` 与 `/etc/issue.net`。
    pub clear_issue: Flag,
    /// 清空 `/etc/motd`。
    #[serde(rename = "clear_motd")]
    pub empty_motd_file: Flag,
    /// `chmod -x /etc/update-motd.d/*`。
    pub disable_update_motd_scripts: Flag,
    /// 自定义头部。
    pub custom_header: MotdHeader,
}

impl Default for Motd {
    fn default() -> Self {
        Self {
            clear_issue: Flag::ON,
            empty_motd_file: Flag::ON,
            disable_update_motd_scripts: Flag::ON,
            custom_header: MotdHeader::default(),
        }
    }
}

/// 模块 17：命令历史审计。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HistoryAudit {
    /// 历史目录。
    pub dir: PlainPath,
    /// 目录权限。
    pub dir_mode: FileMode,
    /// `HISTSIZE` / `HISTFILESIZE`。
    pub histsize: Ranged<1000, 1_000_000>,
    /// 对历史文件 `chattr +a`。
    pub append_only: Flag,
    /// `HISTTIMEFORMAT` 带用户与来源 IP。
    pub timeformat_with_source_ip: Flag,
}

impl Default for HistoryAudit {
    fn default() -> Self {
        Self {
            dir: PlainPath::from_static("/var/log/.history"),
            dir_mode: FileMode::from_static("1777"),
            histsize: Ranged::clamped(100_000),
            append_only: Flag::ON,
            timeformat_with_source_ip: Flag::ON,
        }
    }
}

/// 模块 18：fstrim。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct Fstrim {
    /// `systemctl enable --now fstrim.timer`。
    pub enable_timer: Flag,
}

/// 一个自定义片段。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snippet {
    /// 名称。
    pub name: SafeText,
    /// 正文（以 root 执行）。
    pub body: ShellText,
}

/// 模块 19：自定义片段（唯一的自由脚本入口，默认关闭）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct CustomSnippet {
    /// 片段列表。
    pub snippets: Vec<Snippet>,
}

/// 模块 20：清理。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Cleanup {
    /// 删除 `hosts.equiv`、`.netrc`、`.rhosts`（深度 3）。
    pub remove_trust_files: Flag,
    /// apt clean/autoclean/autoremove。
    pub apt_clean: Flag,
    /// 删除 `/var/log/apt/` 下的文件。
    pub delete_apt_logs: Flag,
    /// 删除轮转日志。
    pub delete_rotated_logs: Flag,
    /// 清空 `*log` 文件。
    pub truncate_logs: Flag,
    /// 清空 `/tmp`。
    pub clear_tmp: Flag,
}

impl Default for Cleanup {
    fn default() -> Self {
        Self {
            remove_trust_files: Flag::ON,
            apt_clean: Flag::ON,
            delete_apt_logs: Flag::ON,
            delete_rotated_logs: Flag::ON,
            truncate_logs: Flag::ON,
            clear_tmp: Flag::ON,
        }
    }
}
