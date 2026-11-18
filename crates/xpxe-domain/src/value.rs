//! 受约束的值类型：所有对外输入都经构造函数校验，反序列化走同一入口。

use serde::de::{Deserializer, Error as DeError};
use serde::{Deserialize, Serialize, Serializer};
use std::fmt::{Display, Formatter};
use std::net::Ipv4Addr;
use std::str::FromStr;

/// 清单与 SSH 公钥解析的辅助实现。
mod lists;
mod ssh;

/// 值类型解析失败：类型名与稳定错误码。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseError {
    /// 值类型名，例如 `Hostname`。
    typ: &'static str,
    /// 稳定错误码，例如 `bad_format`。
    code: &'static str,
}

impl ParseError {
    /// 构造错误。
    ///
    /// # Errors
    ///
    /// 失败时返回对应错误。
    #[must_use]
    pub const fn new(typ: &'static str, code: &'static str) -> Self {
        Self { typ, code }
    }

    /// 值类型名。
    #[must_use]
    pub const fn typ(self) -> &'static str {
        self.typ
    }

    /// 稳定错误码。
    #[must_use]
    pub const fn code(self) -> &'static str {
        self.code
    }
}

impl Display for ParseError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.typ, self.code)
    }
}

impl std::error::Error for ParseError {}

/// 实现 `FromStr` / `Display` / serde，反序列化一律走 `parse`。
macro_rules! impl_string_value {
    ($ty:ty) => {
        impl FromStr for $ty {
            type Err = ParseError;
            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Self::parse(value)
            }
        }
        impl Display for $ty {
            fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.as_str())
            }
        }
        impl Serialize for $ty {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(self.as_str())
            }
        }
        impl<'de> Deserialize<'de> for $ty {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let raw = String::deserialize(deserializer)?;
                raw.parse().map_err(DeError::custom)
            }
        }
    };
}

/// 6 字节以太网 MAC。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MacAddr([u8; 6]);

impl MacAddr {
    /// 解析 `aa:bb:..`、`aa-bb:..` 或 12 位紧凑十六进制，大小写不敏感。
    ///
    /// # Errors
    ///
    /// 长度或字符不合法时返回 `bad_format`。
    pub fn parse(raw: &str) -> Result<Self, ParseError> {
        if raw.contains(':') {
            parse_mac_sep(raw, b':')
        } else if raw.contains('-') {
            parse_mac_sep(raw, b'-')
        } else {
            parse_mac_compact(raw)
        }
    }

    /// 原始 6 字节。
    ///
    /// # Errors
    ///
    /// 失败时返回对应错误。
    #[must_use]
    pub const fn octets(self) -> [u8; 6] {
        self.0
    }

    /// 冒号分隔的小写形式，例如 `52:54:00:77:01:b3`。
    #[must_use]
    pub fn to_colon(self) -> String {
        format_mac(self.0, ':')
    }

    /// iPXE `hexhyp` 小写形式，例如 `52-54-00-77-01-b3`。
    #[must_use]
    pub fn to_hexhyp(self) -> String {
        format_mac(self.0, '-')
    }
}

impl FromStr for MacAddr {
    type Err = ParseError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

impl Display for MacAddr {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.to_colon())
    }
}

impl Serialize for MacAddr {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_colon())
    }
}

impl<'de> Deserialize<'de> for MacAddr {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Self::parse(&raw).map_err(DeError::custom)
    }
}

/// 以给定分隔符格式化 6 字节 MAC 为小写十六进制。
///
/// # Errors
///
/// 失败时返回对应错误。
fn format_mac(octets: [u8; 6], sep: char) -> String {
    let [o0, o1, o2, o3, o4, o5] = octets;
    format!("{o0:02x}{sep}{o1:02x}{sep}{o2:02x}{sep}{o3:02x}{sep}{o4:02x}{sep}{o5:02x}")
}

/// 从带分隔符的文本解析 MAC，总长必须为 17。
///
/// # Errors
///
/// 失败时返回对应错误。
fn parse_mac_sep(raw: &str, sep: u8) -> Result<MacAddr, ParseError> {
    let bytes = raw.as_bytes();
    if bytes.len() != 17 {
        return Err(ParseError::new("MacAddr", "bad_format"));
    }
    let b0 = hex_pair_at(bytes, 0, "MacAddr")?;
    expect_byte(bytes, 2, sep, "MacAddr")?;
    let b1 = hex_pair_at(bytes, 3, "MacAddr")?;
    expect_byte(bytes, 5, sep, "MacAddr")?;
    let b2 = hex_pair_at(bytes, 6, "MacAddr")?;
    expect_byte(bytes, 8, sep, "MacAddr")?;
    let b3 = hex_pair_at(bytes, 9, "MacAddr")?;
    expect_byte(bytes, 11, sep, "MacAddr")?;
    let b4 = hex_pair_at(bytes, 12, "MacAddr")?;
    expect_byte(bytes, 14, sep, "MacAddr")?;
    let b5 = hex_pair_at(bytes, 15, "MacAddr")?;
    Ok(MacAddr([b0, b1, b2, b3, b4, b5]))
}

/// 从 12 位紧凑十六进制解析 MAC。
///
/// # Errors
///
/// 失败时返回对应错误。
fn parse_mac_compact(raw: &str) -> Result<MacAddr, ParseError> {
    let bytes = raw.as_bytes();
    if bytes.len() != 12 {
        return Err(ParseError::new("MacAddr", "bad_format"));
    }
    Ok(MacAddr([
        hex_pair_at(bytes, 0, "MacAddr")?,
        hex_pair_at(bytes, 2, "MacAddr")?,
        hex_pair_at(bytes, 4, "MacAddr")?,
        hex_pair_at(bytes, 6, "MacAddr")?,
        hex_pair_at(bytes, 8, "MacAddr")?,
        hex_pair_at(bytes, 10, "MacAddr")?,
    ]))
}

/// 读取两个十六进制字符并合成一字节。
///
/// # Errors
///
/// 失败时返回对应错误。
fn hex_pair_at(bytes: &[u8], index: usize, typ: &'static str) -> Result<u8, ParseError> {
    let hi = *bytes
        .get(index)
        .ok_or_else(|| ParseError::new(typ, "bad_format"))?;
    let lo_index = index
        .checked_add(1)
        .ok_or_else(|| ParseError::new(typ, "bad_format"))?;
    let lo = *bytes
        .get(lo_index)
        .ok_or_else(|| ParseError::new(typ, "bad_format"))?;
    let high = hex_nibble(hi, typ)?;
    let low = hex_nibble(lo, typ)?;
    high.checked_mul(16)
        .and_then(|value| value.checked_add(low))
        .ok_or_else(|| ParseError::new(typ, "bad_format"))
}

/// 将一个 ASCII 十六进制字符转为 0–15。
///
/// # Errors
///
/// 失败时返回对应错误。
fn hex_nibble(byte: u8, typ: &'static str) -> Result<u8, ParseError> {
    match byte {
        b'0'..=b'9' => byte
            .checked_sub(b'0')
            .ok_or_else(|| ParseError::new(typ, "bad_format")),
        b'a'..=b'f' => byte
            .checked_sub(b'a')
            .and_then(|value| value.checked_add(10))
            .ok_or_else(|| ParseError::new(typ, "bad_format")),
        b'A'..=b'F' => byte
            .checked_sub(b'A')
            .and_then(|value| value.checked_add(10))
            .ok_or_else(|| ParseError::new(typ, "bad_format")),
        _ => Err(ParseError::new(typ, "bad_format")),
    }
}

/// 断言指定位置为给定字节。
///
/// # Errors
///
/// 失败时返回对应错误。
fn expect_byte(
    bytes: &[u8],
    index: usize,
    expected: u8,
    typ: &'static str,
) -> Result<(), ParseError> {
    match bytes.get(index) {
        Some(&found) if found == expected => Ok(()),
        Some(_) | None => Err(ParseError::new(typ, "bad_format")),
    }
}

/// RFC 1123 单标签主机名，1–63 字符，内部一律小写。
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Hostname(String);

impl Hostname {
    /// 解析并规范为小写。
    ///
    /// # Errors
    ///
    /// 空、超长或不符合 LDH 标签规则时返回错误。
    pub fn parse(raw: &str) -> Result<Self, ParseError> {
        if raw.is_empty() {
            return Err(ParseError::new("Hostname", "empty"));
        }
        if raw.len() > 63 {
            return Err(ParseError::new("Hostname", "too_long"));
        }
        if !is_ldh_label(raw) {
            return Err(ParseError::new("Hostname", "bad_format"));
        }
        Ok(Self(raw.to_ascii_lowercase()))
    }

    /// 小写主机名。
    ///
    /// # Errors
    ///
    /// 失败时返回对应错误。
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl_string_value!(Hostname);

/// RFC 1123 FQDN，1–253 字符，标签规则同 [`Hostname`]，存储为无尾点的小写形式。
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Fqdn(String);

impl Fqdn {
    /// 解析 FQDN；允许可选的尾随点，解析后去掉。
    ///
    /// # Errors
    ///
    /// 空、超长或任一标签不合法时返回错误。
    pub fn parse(raw: &str) -> Result<Self, ParseError> {
        parse_fqdn(raw)
    }

    /// 无尾点的小写 FQDN。
    ///
    /// # Errors
    ///
    /// 失败时返回对应错误。
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl_string_value!(Fqdn);

/// 解析 FQDN 的主体，控制圈复杂度。
///
/// # Errors
///
/// 失败时返回对应错误。
fn parse_fqdn(raw: &str) -> Result<Fqdn, ParseError> {
    if raw.is_empty() {
        return Err(ParseError::new("Fqdn", "empty"));
    }
    let trimmed = raw.strip_suffix('.').unwrap_or(raw);
    if trimmed.is_empty() {
        return Err(ParseError::new("Fqdn", "empty"));
    }
    if trimmed.len() > 253 {
        return Err(ParseError::new("Fqdn", "too_long"));
    }
    if trimmed.split('.').all(is_ldh_label) {
        Ok(Fqdn(trimmed.to_ascii_lowercase()))
    } else {
        Err(ParseError::new("Fqdn", "bad_format"))
    }
}

/// LDH 单标签：字母数字开头结尾，中间可含连字符，长度 1–63。
///
/// # Errors
///
/// 失败时返回对应错误。
fn is_ldh_label(label: &str) -> bool {
    if label.is_empty() || label.len() > 63 {
        return false;
    }
    let bytes = label.as_bytes();
    let Some((&first, rest)) = bytes.split_first() else {
        return false;
    };
    let last = rest.last().copied().unwrap_or(first);
    if !is_ascii_alnum(first) || !is_ascii_alnum(last) {
        return false;
    }
    bytes
        .iter()
        .all(|byte| is_ascii_alnum(*byte) || *byte == b'-')
}

/// ASCII 字母或数字。
const fn is_ascii_alnum(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
}

/// IPv4 地址与前缀长度，例如 `192.168.2.10/24`。允许主机地址，不强制网络号对齐。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Ipv4Cidr {
    /// 地址。
    address: Ipv4Addr,
    /// 前缀长度（0–32）。
    prefix: u8,
}

impl Ipv4Cidr {
    /// 解析 `a.b.c.d/prefix`。
    ///
    /// # Errors
    ///
    /// 格式错误或前缀超过 32 时返回错误。
    pub fn parse(raw: &str) -> Result<Self, ParseError> {
        let Some((addr, prefix_text)) = raw.split_once('/') else {
            return Err(ParseError::new("Ipv4Cidr", "bad_format"));
        };
        let address: Ipv4Addr = addr
            .parse()
            .map_err(|_error| ParseError::new("Ipv4Cidr", "bad_format"))?;
        let prefix: u8 = prefix_text
            .parse()
            .map_err(|_error| ParseError::new("Ipv4Cidr", "bad_format"))?;
        if prefix > 32 {
            return Err(ParseError::new("Ipv4Cidr", "bad_prefix"));
        }
        Ok(Self { address, prefix })
    }

    /// 地址。
    ///
    /// # Errors
    ///
    /// 失败时返回对应错误。
    #[must_use]
    pub const fn address(self) -> Ipv4Addr {
        self.address
    }

    /// 前缀长度。
    #[must_use]
    pub const fn prefix(self) -> u8 {
        self.prefix
    }

    /// 规范文本。
    #[must_use]
    pub fn as_str(self) -> String {
        format!("{}/{}", self.address, self.prefix)
    }
}

impl FromStr for Ipv4Cidr {
    type Err = ParseError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

impl Display for Ipv4Cidr {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.address, self.prefix)
    }
}

impl Serialize for Ipv4Cidr {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Ipv4Cidr {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Self::parse(&raw).map_err(DeError::custom)
    }
}

/// Unix 登录名：`^[a-z_][a-z0-9_-]{0,31}$`，拒绝系统保留名（`root` 除外）。
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct UnixUser(String);

impl UnixUser {
    /// 解析用户名。
    ///
    /// # Errors
    ///
    /// 格式不符或落在保留名单中时返回错误。
    pub fn parse(raw: &str) -> Result<Self, ParseError> {
        if raw.is_empty() {
            return Err(ParseError::new("UnixUser", "empty"));
        }
        if raw.len() > 32 {
            return Err(ParseError::new("UnixUser", "too_long"));
        }
        if !is_unix_user_name(raw) {
            return Err(ParseError::new("UnixUser", "bad_format"));
        }
        if raw != "root" && lists::is_reserved_user(raw) {
            return Err(ParseError::new("UnixUser", "reserved"));
        }
        Ok(Self(raw.to_owned()))
    }

    /// 用户名文本。
    ///
    /// # Errors
    ///
    /// 失败时返回对应错误。
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl_string_value!(UnixUser);

/// 是否符合 Unix 用户名正则（不含保留名检查）。
fn is_unix_user_name(raw: &str) -> bool {
    let bytes = raw.as_bytes();
    let Some((&first, rest)) = bytes.split_first() else {
        return false;
    };
    let first_ok = first.is_ascii_lowercase() || first == b'_';
    first_ok
        && rest.iter().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_' || *byte == b'-'
        })
}

/// Debian Policy 软件包名：`^[a-z0-9][a-z0-9+.-]+$`，最长 255。
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PackageName(String);

impl PackageName {
    /// 解析包名。
    ///
    /// # Errors
    ///
    /// 过短、过长或字符不合法时返回错误。
    pub fn parse(raw: &str) -> Result<Self, ParseError> {
        if raw.len() < 2 {
            return Err(ParseError::new("PackageName", "too_short"));
        }
        if raw.len() > 255 {
            return Err(ParseError::new("PackageName", "too_long"));
        }
        let bytes = raw.as_bytes();
        let Some((&first, rest)) = bytes.split_first() else {
            return Err(ParseError::new("PackageName", "too_short"));
        };
        let first_ok = first.is_ascii_lowercase() || first.is_ascii_digit();
        let rest_ok = rest.iter().all(|byte| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || *byte == b'+'
                || *byte == b'.'
                || *byte == b'-'
        });
        if first_ok && rest_ok && !rest.is_empty() {
            Ok(Self(raw.to_owned()))
        } else {
            Err(ParseError::new("PackageName", "bad_format"))
        }
    }

    /// 包名文本。
    ///
    /// # Errors
    ///
    /// 失败时返回对应错误。
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl_string_value!(PackageName);

/// 通过类型白名单与结构校验的 SSH 公钥。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SshPublicKey(String);

impl SshPublicKey {
    /// 解析一行 SSH 公钥。
    ///
    /// # Errors
    ///
    /// 类型不在白名单、base64 无法解码、内嵌类型不一致、RSA 不足 3072 位，
    /// 或注释含非可打印 ASCII / 换行时返回错误。
    pub fn parse(raw: &str) -> Result<Self, ParseError> {
        ssh::parse(raw)
    }

    /// 原始单行公钥文本。
    ///
    /// # Errors
    ///
    /// 失败时返回对应错误。
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl_string_value!(SshPublicKey);

/// 由 [`SshPublicKey::parse`] 调用，仅本模块与 `ssh` 子模块使用。
pub(crate) const fn new_ssh_public_key(raw: String) -> SshPublicKey {
    SshPublicKey(raw)
}

/// 与 `data/zones.txt` 精确匹配的 IANA 时区名。
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TimeZone(String);

impl TimeZone {
    /// 解析时区名，大小写敏感。
    ///
    /// # Errors
    ///
    /// 不在清单中时返回 `unknown`。
    pub fn parse(raw: &str) -> Result<Self, ParseError> {
        if lists::is_listed(lists::zones(), raw) {
            Ok(Self(raw.to_owned()))
        } else {
            Err(ParseError::new("TimeZone", "unknown"))
        }
    }

    /// 时区名。
    ///
    /// # Errors
    ///
    /// 失败时返回对应错误。
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

impl_string_value!(TimeZone);

/// 与 `data/locales.txt`（Debian/glibc `SUPPORTED`）精确匹配的 locale 标识。
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Locale(String);

impl Locale {
    /// 解析 locale，大小写敏感。
    ///
    /// # Errors
    ///
    /// 不在清单中时返回 `unknown`。
    pub fn parse(raw: &str) -> Result<Self, ParseError> {
        if lists::is_listed(lists::locales(), raw) {
            Ok(Self(raw.to_owned()))
        } else {
            Err(ParseError::new("Locale", "unknown"))
        }
    }

    /// locale 标识。
    ///
    /// # Errors
    ///
    /// 失败时返回对应错误。
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

impl_string_value!(Locale);

/// 仅 `http`/`https`、必须有主机、不含空白与控制字符的 URL。
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct HttpUrl(String);

impl HttpUrl {
    /// 解析 URL，方案名规范为小写。
    ///
    /// # Errors
    ///
    /// 方案不受支持、缺少主机、含空白/控制字符或过长时返回错误。
    pub fn parse(raw: &str) -> Result<Self, ParseError> {
        parse_http_url(raw)
    }

    /// 规范化后的 URL。
    ///
    /// # Errors
    ///
    /// 失败时返回对应错误。
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl_string_value!(HttpUrl);

/// 单行、可打印 UTF-8 文本，最长 [`SafeText::MAX_LEN`] 字节。
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SafeText(String);

impl SafeText {
    /// 最大字节长度。
    pub const MAX_LEN: usize = 1024;

    /// 解析安全单行文本。
    ///
    /// # Errors
    ///
    /// 空、超长或含控制字符时返回错误。
    pub fn parse(raw: &str) -> Result<Self, ParseError> {
        if raw.is_empty() {
            return Err(ParseError::new("SafeText", "empty"));
        }
        if raw.len() > Self::MAX_LEN {
            return Err(ParseError::new("SafeText", "too_long"));
        }
        if raw.chars().any(char::is_control) {
            return Err(ParseError::new("SafeText", "control_char"));
        }
        Ok(Self(raw.to_owned()))
    }

    /// 文本内容。
    ///
    /// # Errors
    ///
    /// 失败时返回对应错误。
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl_string_value!(SafeText);

/// 装机引导令牌（base64url，8–64 字符）。
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BootToken(String);

impl BootToken {
    /// 解析令牌。
    ///
    /// # Errors
    ///
    /// 长度或字符集不合法时返回错误。
    pub fn parse(raw: &str) -> Result<Self, ParseError> {
        parse_token(raw, "BootToken", 8, 64)
    }

    /// 令牌文本。
    ///
    /// # Errors
    ///
    /// 失败时返回对应错误。
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl_string_value!(BootToken);

/// 解析 `http`/`https` URL。
///
/// # Errors
///
/// 失败时返回对应错误。
fn parse_http_url(raw: &str) -> Result<HttpUrl, ParseError> {
    if raw
        .chars()
        .any(|ch| ch.is_control() || ch.is_whitespace() || ch == '\\')
    {
        return Err(ParseError::new("HttpUrl", "bad_char"));
    }
    if raw.len() > 2048 {
        return Err(ParseError::new("HttpUrl", "too_long"));
    }
    let Some(scheme_end) = raw.find("://") else {
        return Err(ParseError::new("HttpUrl", "bad_scheme"));
    };
    let scheme_raw = raw
        .get(..scheme_end)
        .ok_or(ParseError::new("HttpUrl", "bad_scheme"))?;
    let rest_at = scheme_end
        .checked_add(3)
        .ok_or(ParseError::new("HttpUrl", "bad_scheme"))?;
    let rest = raw
        .get(rest_at..)
        .ok_or(ParseError::new("HttpUrl", "bad_scheme"))?;
    let scheme = scheme_raw.to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return Err(ParseError::new("HttpUrl", "bad_scheme"));
    }
    let authority = authority_of(rest)?;
    if authority.is_empty() || authority.contains('@') {
        return Err(ParseError::new("HttpUrl", "bad_host"));
    }
    let host = host_of(authority)?;
    if host.is_empty() {
        return Err(ParseError::new("HttpUrl", "bad_host"));
    }
    Ok(HttpUrl(format!("{scheme}://{rest}")))
}

/// 取出 URL 的 authority（主机\[:端口\]）。
///
/// # Errors
///
/// 失败时返回对应错误。
fn authority_of(rest: &str) -> Result<&str, ParseError> {
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    rest.get(..end)
        .ok_or(ParseError::new("HttpUrl", "bad_host"))
}

/// 从 authority 中取出主机；IPv6 必须用方括号。
///
/// # Errors
///
/// 失败时返回对应错误。
fn host_of(authority: &str) -> Result<&str, ParseError> {
    if let Some(inner) = authority.strip_prefix('[') {
        let Some(end) = inner.find(']') else {
            return Err(ParseError::new("HttpUrl", "bad_host"));
        };
        return inner
            .get(..end)
            .ok_or(ParseError::new("HttpUrl", "bad_host"));
    }
    let end = authority.find(':').unwrap_or(authority.len());
    authority
        .get(..end)
        .ok_or(ParseError::new("HttpUrl", "bad_host"))
}

/// 解析限定字符集与长度的令牌。
///
/// # Errors
///
/// 失败时返回对应错误。
fn parse_token(
    raw: &str,
    typ: &'static str,
    min: usize,
    max: usize,
) -> Result<BootToken, ParseError> {
    if raw.len() < min {
        return Err(ParseError::new(typ, "too_short"));
    }
    if raw.len() > max {
        return Err(ParseError::new(typ, "too_long"));
    }
    let ok = raw
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_');
    if ok {
        Ok(BootToken(raw.to_owned()))
    } else {
        Err(ParseError::new(typ, "bad_format"))
    }
}

/// 清单加载（供子模块与本模块使用）。
pub(crate) static ZONES_RAW: &str = include_str!("../data/zones.txt");
/// locale 清单原文。
pub(crate) static LOCALES_RAW: &str = include_str!("../data/locales.txt");
/// 保留用户名原文。
pub(crate) static RESERVED_USERS_RAW: &str = include_str!("../data/reserved-users.txt");

#[cfg(test)]
mod tests;
