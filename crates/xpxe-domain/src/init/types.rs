//! 初始化模块专用的值类型：开关、整数范围、单元名、文件名、相对路径、sysctl 键与片段正文。

use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};

use crate::value::ParseError;

/// 布尔开关（用新类型承载，避免结构体里堆叠裸 `bool`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(transparent)]
pub struct Flag(bool);

impl Flag {
    /// 开。
    pub const ON: Self = Self(true);
    /// 关。
    pub const OFF: Self = Self(false);

    /// 由布尔值构造。
    #[must_use]
    pub const fn new(value: bool) -> Self {
        Self(value)
    }

    /// 是否为开。
    #[must_use]
    pub const fn is_on(self) -> bool {
        self.0
    }
}

/// 闭区间 `MIN..=MAX` 内的整数。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "u64", into = "u64")]
pub struct Ranged<const MIN: u64, const MAX: u64>(u64);

impl<const MIN: u64, const MAX: u64> Ranged<MIN, MAX> {
    /// 校验并构造。
    ///
    /// # Errors
    ///
    /// 超出范围时返回错误。
    pub const fn new(value: u64) -> Result<Self, ParseError> {
        if value < MIN || value > MAX {
            Err(ParseError::new("Ranged", "out_of_range"))
        } else {
            Ok(Self(value))
        }
    }

    /// 编译期常量构造：超出范围时取最接近的边界（仅用于内置默认值）。
    #[must_use]
    pub const fn clamped(value: u64) -> Self {
        if value < MIN {
            Self(MIN)
        } else if value > MAX {
            Self(MAX)
        } else {
            Self(value)
        }
    }

    /// 数值。
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl<const MIN: u64, const MAX: u64> TryFrom<u64> for Ranged<MIN, MAX> {
    type Error = ParseError;
    fn try_from(value: u64) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl<const MIN: u64, const MAX: u64> From<Ranged<MIN, MAX>> for u64 {
    fn from(value: Ranged<MIN, MAX>) -> Self {
        value.0
    }
}

impl<const MIN: u64, const MAX: u64> Display for Ranged<MIN, MAX> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// 为 `String` 新类型实现 `TryFrom<String>` / `Into<String>` / `Display`。
macro_rules! string_newtype {
    ($ty:ident) => {
        impl TryFrom<String> for $ty {
            type Error = ParseError;
            fn try_from(value: String) -> Result<Self, Self::Error> {
                Self::parse(&value)
            }
        }
        impl From<$ty> for String {
            fn from(value: $ty) -> Self {
                value.0
            }
        }
        impl Display for $ty {
            fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }
        impl $ty {
            /// 文本内容。
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
    };
}

/// 内置默认值构造：不校验，合法性由“默认计划序列化后可重新解析”的测试保证。
macro_rules! static_ctor {
    ($ty:ident) => {
        impl $ty {
            /// 内置默认值构造（不校验）。
            pub(crate) fn from_static(raw: &str) -> Self {
                Self(raw.to_owned())
            }
        }
    };
}

/// 是否只由 ASCII 字母数字与给定标点组成。
fn only_chars(raw: &str, extra: &[u8]) -> bool {
    raw.bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || extra.contains(&byte))
}

/// systemd 单元名：`[A-Za-z0-9:_.@-]`，1–255 字节，不以 `-` 或 `.` 开头。
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct UnitName(String);

impl UnitName {
    /// 解析单元名。
    ///
    /// # Errors
    ///
    /// 空、过长或含非法字符时返回错误。
    pub fn parse(raw: &str) -> Result<Self, ParseError> {
        if raw.is_empty() || raw.len() > 255 {
            return Err(ParseError::new("UnitName", "bad_length"));
        }
        if raw.starts_with('-') || raw.starts_with('.') || !only_chars(raw, b":_.@-") {
            return Err(ParseError::new("UnitName", "bad_format"));
        }
        Ok(Self(raw.to_owned()))
    }
}

string_newtype!(UnitName);
static_ctor!(UnitName);

/// 单段文件名：`[A-Za-z0-9._-]`，1–128 字节，不以 `.` 或 `-` 开头。
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct SafeFileName(String);

impl SafeFileName {
    /// 解析文件名。
    ///
    /// # Errors
    ///
    /// 空、过长、以 `.`/`-` 开头或含非法字符时返回错误。
    pub fn parse(raw: &str) -> Result<Self, ParseError> {
        if raw.is_empty() || raw.len() > 128 {
            return Err(ParseError::new("SafeFileName", "bad_length"));
        }
        if raw.starts_with('.') || raw.starts_with('-') || !only_chars(raw, b"._-") {
            return Err(ParseError::new("SafeFileName", "bad_format"));
        }
        Ok(Self(raw.to_owned()))
    }
}

string_newtype!(SafeFileName);
static_ctor!(SafeFileName);

/// 家目录内的相对路径：段由 `[A-Za-z0-9._-]` 组成，不含 `.`/`..` 段，不以 `/` 开头。
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct RelPath(String);

impl RelPath {
    /// 解析相对路径。
    ///
    /// # Errors
    ///
    /// 空、绝对路径、含点段或非法字符时返回错误。
    pub fn parse(raw: &str) -> Result<Self, ParseError> {
        if raw.is_empty() || raw.len() > 255 {
            return Err(ParseError::new("RelPath", "bad_length"));
        }
        let bad_segment = raw
            .split('/')
            .any(|seg| seg.is_empty() || seg == "." || seg == ".." || !only_chars(seg, b"._-"));
        if bad_segment {
            return Err(ParseError::new("RelPath", "bad_format"));
        }
        Ok(Self(raw.to_owned()))
    }
}

string_newtype!(RelPath);
static_ctor!(RelPath);

/// sysctl 键：点分段，每段 `[a-z0-9_-]`，可含 `/`（接口名），1–128 字节。
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct SysctlKey(String);

impl SysctlKey {
    /// 解析 sysctl 键。
    ///
    /// # Errors
    ///
    /// 格式不合法时返回错误。
    pub fn parse(raw: &str) -> Result<Self, ParseError> {
        if raw.is_empty() || raw.len() > 128 {
            return Err(ParseError::new("SysctlKey", "bad_length"));
        }
        let ok = raw.split('.').all(|seg| {
            !seg.is_empty()
                && seg.bytes().all(|byte| {
                    byte.is_ascii_lowercase()
                        || byte.is_ascii_digit()
                        || byte == b'_'
                        || byte == b'-'
                        || byte == b'/'
                })
        });
        if ok && raw.contains('.') {
            Ok(Self(raw.to_owned()))
        } else {
            Err(ParseError::new("SysctlKey", "bad_format"))
        }
    }
}

string_newtype!(SysctlKey);

/// sysctl 取值：可打印 ASCII 与空格，1–256 字节，不含引号、反斜杠、`$`、反引号与 `#`。
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct SysctlValue(String);

impl SysctlValue {
    /// 解析取值。
    ///
    /// # Errors
    ///
    /// 空、过长或含非法字符时返回错误。
    pub fn parse(raw: &str) -> Result<Self, ParseError> {
        if raw.is_empty() || raw.len() > 256 || raw.trim() != raw {
            return Err(ParseError::new("SysctlValue", "bad_length"));
        }
        let ok = raw.bytes().all(|byte| {
            (byte.is_ascii_graphic() || byte == b' ')
                && !matches!(byte, b'\'' | b'"' | b'\\' | b'$' | b'`' | b'#')
        });
        if ok {
            Ok(Self(raw.to_owned()))
        } else {
            Err(ParseError::new("SysctlValue", "bad_format"))
        }
    }
}

string_newtype!(SysctlValue);

/// 自定义片段正文：UTF-8，最多 64 KiB，不含 NUL。
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ShellText(String);

impl ShellText {
    /// 最大字节数。
    pub const MAX_LEN: usize = 64 * 1024;

    /// 解析片段正文。
    ///
    /// # Errors
    ///
    /// 空、超长或含 NUL 时返回错误。
    pub fn parse(raw: &str) -> Result<Self, ParseError> {
        if raw.trim().is_empty() {
            return Err(ParseError::new("ShellText", "empty"));
        }
        if raw.len() > Self::MAX_LEN {
            return Err(ParseError::new("ShellText", "too_long"));
        }
        if raw.contains('\0') {
            return Err(ParseError::new("ShellText", "nul_byte"));
        }
        Ok(Self(raw.to_owned()))
    }
}

string_newtype!(ShellText);

/// 只含 `[A-Za-z0-9/._+-]` 的绝对路径（可安全写入 sudoers、systemd 与 shell 配置）。
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct PlainPath(String);

impl PlainPath {
    /// 解析路径。
    ///
    /// # Errors
    ///
    /// 非绝对路径、含空段/点段或非法字符时返回错误。
    pub fn parse(raw: &str) -> Result<Self, ParseError> {
        if raw.len() < 2 || raw.len() > 255 {
            return Err(ParseError::new("PlainPath", "bad_length"));
        }
        let Some(rest) = raw.strip_prefix('/') else {
            return Err(ParseError::new("PlainPath", "not_absolute"));
        };
        let bad = rest
            .split('/')
            .any(|seg| seg.is_empty() || seg == "." || seg == ".." || !only_chars(seg, b"._+-"));
        if bad {
            return Err(ParseError::new("PlainPath", "bad_format"));
        }
        Ok(Self(raw.to_owned()))
    }
}

string_newtype!(PlainPath);
static_ctor!(PlainPath);

/// 环境变量名：`[A-Z_][A-Z0-9_]*`，最长 64。
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct EnvName(String);

impl EnvName {
    /// 解析变量名。
    ///
    /// # Errors
    ///
    /// 格式不合法时返回错误。
    pub fn parse(raw: &str) -> Result<Self, ParseError> {
        let first_ok = raw
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_uppercase() || byte == b'_');
        let rest_ok = raw
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_');
        if first_ok && rest_ok && raw.len() <= 64 {
            Ok(Self(raw.to_owned()))
        } else {
            Err(ParseError::new("EnvName", "bad_format"))
        }
    }
}

string_newtype!(EnvName);
static_ctor!(EnvName);

/// 八进制权限：3–4 位 `0-7`。
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct FileMode(String);

impl FileMode {
    /// 解析权限。
    ///
    /// # Errors
    ///
    /// 不是 3–4 位八进制数字时返回错误。
    pub fn parse(raw: &str) -> Result<Self, ParseError> {
        let ok =
            (3..=4).contains(&raw.len()) && raw.bytes().all(|byte| (b'0'..=b'7').contains(&byte));
        if ok {
            Ok(Self(raw.to_owned()))
        } else {
            Err(ParseError::new("FileMode", "bad_format"))
        }
    }
}

string_newtype!(FileMode);
static_ctor!(FileMode);
