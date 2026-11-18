//! Debian `xkb-keymap` 键盘布局标识。

use serde::de::{Deserializer, Error as DeError};
use serde::{Deserialize, Serialize, Serializer};
use std::fmt::{Display, Formatter};
use std::str::FromStr;
use std::sync::LazyLock;

use crate::value::ParseError;

/// 键盘布局清单原文。
const KEYMAPS_RAW: &str = include_str!("../../data/keymaps.txt");

/// 已排序的布局名。
static KEYMAPS: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    KEYMAPS_RAW
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect()
});

/// 与 `data/keymaps.txt`（XKB `evdev` 布局，另含 Debian 常用别名 `uk`）精确匹配。
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Keymap(String);

impl Keymap {
    /// 解析布局名，大小写敏感。
    ///
    /// # Errors
    ///
    /// 不在清单中时返回 `unknown`。
    pub fn parse(raw: &str) -> Result<Self, ParseError> {
        if KEYMAPS.binary_search(&raw).is_ok() {
            Ok(Self(raw.to_owned()))
        } else {
            Err(ParseError::new("Keymap", "unknown"))
        }
    }

    /// 布局名。
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// 仅用于编译期已知的合法默认值。
    #[must_use]
    pub(crate) fn from_verified(raw: &str) -> Self {
        Self(raw.to_owned())
    }
}

impl FromStr for Keymap {
    type Err = ParseError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

impl Display for Keymap {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for Keymap {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for Keymap {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        raw.parse().map_err(DeError::custom)
    }
}
