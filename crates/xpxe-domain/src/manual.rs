//! 手动 ISO 网络引导菜单（Step 14）。
//!
//! 菜单是一个全局配置文档（版本链、可回滚、审计），与节点状态机完全隔离：
//! 命中菜单与选择条目只记观测与审计，不产生状态转换。

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::versioning::{ConfigDocument, ConfigError, ConfigKind, FieldError, field_error};
use crate::{MacAddr, Sha256Hex};

/// 条目数上限。
pub const MAX_ENTRIES: usize = 64;
/// MAC 白名单上限。
pub const MAX_MACS: usize = 4096;
/// 倒计时上限（秒）。
pub const MAX_TIMEOUT_S: u32 = 3600;
/// 内核命令行长度上限。
pub const MAX_CMDLINE: usize = 1024;
/// 路径长度上限。
pub const MAX_PATH: usize = 255;
/// 标题长度上限（字符）。
pub const MAX_TITLE: usize = 64;

/// 条目适用的固件。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryArch {
    /// 不限。
    #[default]
    Any,
    /// 仅 BIOS。
    Bios,
    /// 仅 UEFI。
    Uefi,
}

impl EntryArch {
    /// 是否适用于给定固件（`uefi = true` 为 UEFI）。
    #[must_use]
    pub const fn matches(self, uefi: bool) -> bool {
        match self {
            Self::Any => true,
            Self::Bios => !uefi,
            Self::Uefi => uefi,
        }
    }
}

/// 条目类型。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum EntryKind {
    /// 整盘 ISO 经 HTTP 映射为块设备。
    Sanboot {
        /// ISO 的 SHA-256。
        iso: Sha256Hex,
    },
    /// 从 ISO 抽取内核与 initrd。
    KernelInitrd {
        /// ISO 的 SHA-256。
        iso: Sha256Hex,
        /// ISO 内内核路径。
        kernel_path: String,
        /// ISO 内 initrd 路径。
        initrd_path: String,
        /// 内核命令行（白名单字符）。
        #[serde(default)]
        cmdline: String,
        /// 是否把整份 ISO 打进 initrd（PVE 系）。
        #[serde(default)]
        embed_iso: bool,
        /// 嵌入时 ISO 在 initrd 中的文件名，默认 `/proxmox.iso`。
        #[serde(default)]
        iso_name: Option<String>,
    },
    /// 链到另一个 iPXE 脚本。
    Chain {
        /// 以 `/` 开头的同源路径，或命中白名单前缀的 `http(s)://` URL。
        url: String,
    },
    /// 本地启动。
    Exit,
}

impl EntryKind {
    /// 引用的 ISO。
    #[must_use]
    pub const fn iso(&self) -> Option<&Sha256Hex> {
        match self {
            Self::Sanboot { iso } | Self::KernelInitrd { iso, .. } => Some(iso),
            Self::Chain { .. } | Self::Exit => None,
        }
    }
}

/// 一个菜单条目。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BootMenuEntry {
    /// 稳定 ID（`[a-z0-9_-]{1,32}`）。
    pub id: String,
    /// 标题。
    pub title: String,
    /// 是否启用。
    #[serde(default = "enabled_default")]
    pub enabled: bool,
    /// 适用固件。
    #[serde(default)]
    pub arch: EntryArch,
    /// 类型化定义。
    pub kind: EntryKind,
}

/// serde 默认启用。
const fn enabled_default() -> bool {
    true
}

/// 手动引导菜单文档。
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BootMenu {
    /// 总开关（默认关闭）。
    #[serde(default)]
    pub enabled: bool,
    /// 对所有未知（未登记）机器显示。
    #[serde(default)]
    pub show_unknown: bool,
    /// MAC 白名单。
    #[serde(default)]
    pub allow_macs: Vec<MacAddr>,
    /// 默认条目 ID。
    #[serde(default)]
    pub default_entry: Option<String>,
    /// 倒计时秒数（0 表示一直等待）。
    #[serde(default)]
    pub timeout_s: u32,
    /// `chain` 条目允许的 URL 前缀（同源路径无需列出）。
    #[serde(default)]
    pub chain_prefixes: Vec<String>,
    /// 有序条目。
    #[serde(default)]
    pub entries: Vec<BootMenuEntry>,
}

impl BootMenu {
    /// 菜单是否对该 MAC 显示（`known` 表示该 MAC 已登记为节点）。
    #[must_use]
    pub fn shows_for(&self, mac: MacAddr, known: bool) -> bool {
        self.enabled && (self.allow_macs.contains(&mac) || (self.show_unknown && !known))
    }

    /// 按 ID 找到启用的条目。
    #[must_use]
    pub fn entry(&self, id: &str) -> Option<&BootMenuEntry> {
        self.entries
            .iter()
            .find(|entry| entry.enabled && entry.id == id)
    }

    /// 引用了 `iso` 的条目 ID。
    #[must_use]
    pub fn referencing(&self, iso: &Sha256Hex) -> Vec<String> {
        self.entries
            .iter()
            .filter(|entry| entry.kind.iso() == Some(iso))
            .map(|entry| entry.id.clone())
            .collect()
    }
}

/// 内核命令行白名单字符。
#[must_use]
pub fn cmdline_char_ok(item: char) -> bool {
    item.is_ascii_alphanumeric() || " ._,:=/+@%-~".contains(item)
}

/// ISO 内路径白名单字符。
fn path_char_ok(item: char) -> bool {
    item.is_ascii_alphanumeric() || "._/+-~".contains(item)
}

/// URL 白名单字符（不含空白、引号与 iPXE 展开符 `$`）。
fn url_char_ok(item: char) -> bool {
    item.is_ascii_alphanumeric() || "._/+-~:?&=%#@!,;".contains(item)
}

/// 校验 ISO 内绝对路径。
fn check_path(errors: &mut Vec<FieldError>, at: &str, path: &str) {
    let ok = path.starts_with('/')
        && path.len() <= MAX_PATH
        && path.chars().all(path_char_ok)
        && !path.split('/').any(|part| part == "..");
    if !ok {
        errors.push(field_error(
            at,
            "bad_path",
            "须为 / 开头、无 .. 的 ISO 内路径，且只含字母数字与 ._/+-~",
        ));
    }
}

/// 校验 `chain` URL：同源路径或白名单前缀。
fn check_chain(errors: &mut Vec<FieldError>, at: &str, url: &str, prefixes: &[String]) {
    let shaped =
        !url.is_empty() && url.len() <= MAX_PATH.saturating_mul(4) && url.chars().all(url_char_ok);
    let same_origin = url.starts_with('/') && !url.starts_with("//");
    let listed = prefixes
        .iter()
        .any(|prefix| url.starts_with(prefix.as_str()));
    if !shaped {
        errors.push(field_error(at, "bad_url", "URL 含不允许的字符或过长"));
        return;
    }
    if !(same_origin || listed) {
        errors.push(field_error(
            at,
            "chain_not_allowed",
            "chain 只允许同源路径或白名单前缀",
        ));
    }
}

/// 校验条目类型字段。
fn check_kind(errors: &mut Vec<FieldError>, at: &str, kind: &EntryKind, prefixes: &[String]) {
    match kind {
        EntryKind::KernelInitrd {
            kernel_path,
            initrd_path,
            cmdline,
            iso_name,
            ..
        } => {
            check_path(errors, &format!("{at}/kernel_path"), kernel_path);
            check_path(errors, &format!("{at}/initrd_path"), initrd_path);
            if let Some(name) = iso_name {
                check_path(errors, &format!("{at}/iso_name"), name);
            }
            if cmdline.len() > MAX_CMDLINE || !cmdline.chars().all(cmdline_char_ok) {
                errors.push(field_error(
                    &format!("{at}/cmdline"),
                    "bad_cmdline",
                    "cmdline 只允许字母数字、空格与 ._,:=/+@%-~，且不超过 1024 字节",
                ));
            }
        }
        EntryKind::Chain { url } => check_chain(errors, &format!("{at}/url"), url, prefixes),
        EntryKind::Sanboot { .. } | EntryKind::Exit => {}
    }
}

/// 校验单个条目（ID、标题与类型）。
fn check_entry(
    errors: &mut Vec<FieldError>,
    index: usize,
    entry: &BootMenuEntry,
    prefixes: &[String],
) {
    let at = format!("/entries/{index}");
    let id_ok = (1..=32).contains(&entry.id.len())
        && entry.id.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_' || byte == b'-'
        });
    if !id_ok {
        errors.push(field_error(
            &format!("{at}/id"),
            "bad_id",
            "ID 须为 1~32 个小写字母、数字、_ 或 -",
        ));
    }
    let title_len = entry.title.chars().count();
    if title_len == 0 || title_len > MAX_TITLE || entry.title.chars().any(char::is_control) {
        errors.push(field_error(
            &format!("{at}/title"),
            "bad_title",
            "标题须为 1~64 个非控制字符",
        ));
    }
    check_kind(errors, &format!("{at}/kind"), &entry.kind, prefixes);
}

/// 校验菜单级字段。
fn check_menu(errors: &mut Vec<FieldError>, menu: &BootMenu) {
    if menu.entries.len() > MAX_ENTRIES {
        errors.push(field_error("/entries", "too_many", "条目最多 64 个"));
    }
    if menu.allow_macs.len() > MAX_MACS {
        errors.push(field_error(
            "/allow_macs",
            "too_many",
            "MAC 白名单最多 4096 个",
        ));
    }
    if menu.timeout_s > MAX_TIMEOUT_S {
        errors.push(field_error(
            "/timeout_s",
            "out_of_range",
            "倒计时不超过 3600 秒",
        ));
    }
    for (index, prefix) in menu.chain_prefixes.iter().enumerate() {
        let ok = (prefix.starts_with("http://") || prefix.starts_with("https://"))
            && prefix.chars().all(url_char_ok)
            && prefix.matches('/').count() >= 3;
        if !ok {
            errors.push(field_error(
                &format!("/chain_prefixes/{index}"),
                "bad_prefix",
                "前缀须为含主机与路径的 http(s):// URL，例如 http://host/",
            ));
        }
    }
    if let Some(default) = &menu.default_entry
        && menu.entry(default).is_none()
    {
        errors.push(field_error(
            "/default_entry",
            "unknown_entry",
            "默认项须为已启用的条目 ID",
        ));
    }
}

impl ConfigDocument for BootMenu {
    const KIND: ConfigKind = ConfigKind::BootMenu;
    const SCHEMA: u32 = 1;

    fn validate(&self) -> Result<(), Vec<FieldError>> {
        let mut errors = Vec::new();
        check_menu(&mut errors, self);
        let mut seen = HashSet::new();
        for (index, entry) in self.entries.iter().enumerate() {
            check_entry(&mut errors, index, entry, &self.chain_prefixes);
            if !seen.insert(entry.id.as_str()) {
                errors.push(field_error(
                    &format!("/entries/{index}/id"),
                    "duplicate_id",
                    "条目 ID 重复",
                ));
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    fn migrate(from_schema: u32, value: Value) -> Result<Self, ConfigError> {
        crate::versioning::migrate_current(from_schema, value)
    }
}

#[cfg(test)]
mod tests;
