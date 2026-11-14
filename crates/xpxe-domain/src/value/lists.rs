//! IANA 时区、Debian locale 与系统保留用户名清单。

use std::sync::LazyLock;

use super::{LOCALES_RAW, RESERVED_USERS_RAW, ZONES_RAW};

/// 时区清单。
static ZONES: LazyLock<Vec<&'static str>> = LazyLock::new(|| load_lines(ZONES_RAW));
/// locale 清单。
static LOCALES: LazyLock<Vec<&'static str>> = LazyLock::new(|| load_lines(LOCALES_RAW));
/// 保留用户名清单。
static RESERVED_USERS: LazyLock<Vec<&'static str>> =
    LazyLock::new(|| load_lines(RESERVED_USERS_RAW));

/// 从原文加载非空、非注释行（文件已排序）。
fn load_lines(raw: &'static str) -> Vec<&'static str> {
    raw.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect()
}

/// 时区清单切片。
pub(crate) fn zones() -> &'static [&'static str] {
    ZONES.as_slice()
}

/// locale 清单切片。
pub(crate) fn locales() -> &'static [&'static str] {
    LOCALES.as_slice()
}

/// `needle` 是否出现在已排序的 `haystack` 中。
pub(crate) fn is_listed(haystack: &[&str], needle: &str) -> bool {
    haystack.binary_search(&needle).is_ok()
}

/// 是否为系统保留用户名。
pub(crate) fn is_reserved_user(name: &str) -> bool {
    is_listed(RESERVED_USERS.as_slice(), name)
}
