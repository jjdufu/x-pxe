//! tasksel 任务、升级策略与安装器调试。

use serde::{Deserialize, Serialize};

/// Debian tasksel 任务名。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TaskName {
    /// 标准系统工具。
    Standard,
    /// OpenSSH 服务器。
    SshServer,
    /// Web 服务器。
    WebServer,
    /// 邮件服务器。
    MailServer,
    /// 数据库服务器。
    DatabaseServer,
    /// 打印服务器。
    PrintServer,
    /// 笔记本工具。
    Laptop,
    /// 默认桌面。
    Desktop,
    /// `GNOME`。
    GnomeDesktop,
    /// `Xfce`。
    XfceDesktop,
    /// `KDE`。
    KdeDesktop,
    /// `LXDE`。
    LxdeDesktop,
    /// `LXQt`。
    LxqtDesktop,
    /// `Cinnamon`。
    CinnamonDesktop,
    /// `MATE` 桌面。
    MateDesktop,
}

impl TaskName {
    /// tasksel 标识。
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::SshServer => "ssh-server",
            Self::WebServer => "web-server",
            Self::MailServer => "mail-server",
            Self::DatabaseServer => "database-server",
            Self::PrintServer => "print-server",
            Self::Laptop => "laptop",
            Self::Desktop => "desktop",
            Self::GnomeDesktop => "gnome-desktop",
            Self::XfceDesktop => "xfce-desktop",
            Self::KdeDesktop => "kde-desktop",
            Self::LxdeDesktop => "lxde-desktop",
            Self::LxqtDesktop => "lxqt-desktop",
            Self::CinnamonDesktop => "cinnamon-desktop",
            Self::MateDesktop => "mate-desktop",
        }
    }
}

/// 任务集合；默认 `ssh-server` 与 `standard`。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskSet {
    /// 任务列表。
    #[serde(default = "default_tasks")]
    pub items: Vec<TaskName>,
}

impl Default for TaskSet {
    fn default() -> Self {
        Self {
            items: default_tasks(),
        }
    }
}

impl TaskSet {
    /// 去重后按枚举序排列的任务。
    #[must_use]
    pub fn unique(&self) -> Vec<TaskName> {
        unique_tasks(&self.items)
    }
}

/// 默认任务。
fn default_tasks() -> Vec<TaskName> {
    vec![TaskName::SshServer, TaskName::Standard]
}

/// 任务去重。
fn unique_tasks(items: &[TaskName]) -> Vec<TaskName> {
    let all = all_tasks();
    let mut seen = [false; 15];
    for item in items {
        if let Some(slot) = seen.get_mut(task_index(*item)) {
            *slot = true;
        }
    }
    all.into_iter()
        .zip(seen)
        .filter_map(|(task, keep)| keep.then_some(task))
        .collect()
}

/// 全部任务（与下标对应）。
const fn all_tasks() -> [TaskName; 15] {
    [
        TaskName::Standard,
        TaskName::SshServer,
        TaskName::WebServer,
        TaskName::MailServer,
        TaskName::DatabaseServer,
        TaskName::PrintServer,
        TaskName::Laptop,
        TaskName::Desktop,
        TaskName::GnomeDesktop,
        TaskName::XfceDesktop,
        TaskName::KdeDesktop,
        TaskName::LxdeDesktop,
        TaskName::LxqtDesktop,
        TaskName::CinnamonDesktop,
        TaskName::MateDesktop,
    ]
}

/// 任务下标。
const fn task_index(task: TaskName) -> usize {
    match task {
        TaskName::Standard => 0,
        TaskName::SshServer => 1,
        TaskName::WebServer => 2,
        TaskName::MailServer => 3,
        TaskName::DatabaseServer => 4,
        TaskName::PrintServer => 5,
        TaskName::Laptop => 6,
        TaskName::Desktop => 7,
        TaskName::GnomeDesktop => 8,
        TaskName::XfceDesktop => 9,
        TaskName::KdeDesktop => 10,
        TaskName::LxdeDesktop => 11,
        TaskName::LxqtDesktop => 12,
        TaskName::CinnamonDesktop => 13,
        TaskName::MateDesktop => 14,
    }
}

/// `pkgsel/upgrade` 策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Upgrade {
    /// 不升级。
    #[default]
    None,
    /// `safe-upgrade`。
    #[serde(rename = "safe_upgrade")]
    Safe,
    /// `full-upgrade`。
    #[serde(rename = "full_upgrade")]
    Full,
}

impl Upgrade {
    /// preseed 取值。
    #[must_use]
    pub const fn as_preseed(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Safe => "safe-upgrade",
            Self::Full => "full-upgrade",
        }
    }
}

/// 安装器调试。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InstallerDebug {
    /// 关闭。
    #[default]
    Off,
    /// `DEBCONF_DEBUG` 级别 1–5。
    Debconf {
        /// 调试级别。
        level: u8,
    },
}

impl InstallerDebug {
    /// 级别是否合法。
    #[must_use]
    pub const fn is_valid(self) -> bool {
        match self {
            Self::Off => true,
            Self::Debconf { level } => matches!(level, 1..=5),
        }
    }

    /// `DEBCONF_DEBUG` 取值（仅调试时）。
    #[must_use]
    pub const fn debconf_value(self) -> Option<u8> {
        match self {
            Self::Off => None,
            Self::Debconf { level } => Some(level),
        }
    }
}
