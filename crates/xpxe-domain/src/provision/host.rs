//! 镜像主机、NTP、控制台与绝对路径。

use std::fmt::{Display, Formatter};
use std::net::Ipv4Addr;

use serde::{Deserialize, Serialize};

use crate::value::{Fqdn, HttpUrl, ParseError};

/// `host` 或 `host:port`，主机为 IPv4 或 FQDN。
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct HostPort(String);

impl HostPort {
    /// 解析 `host` 或 `host:port`。
    ///
    /// # Errors
    ///
    /// 含非法字符、主机无法解析或端口不在 1–65535 时返回错误。
    pub fn parse(raw: &str) -> Result<Self, ParseError> {
        parse_host_port(raw)
    }

    /// 规范文本。
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for HostPort {
    type Error = ParseError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

impl From<HostPort> for String {
    fn from(value: HostPort) -> Self {
        value.0
    }
}

impl Display for HostPort {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// 解析主机与可选端口。
///
/// # Errors
///
/// 格式不合法时返回错误。
fn parse_host_port(raw: &str) -> Result<HostPort, ParseError> {
    if raw.is_empty() {
        return Err(ParseError::new("HostPort", "empty"));
    }
    if raw.len() > 253 {
        return Err(ParseError::new("HostPort", "too_long"));
    }
    if raw.chars().any(|ch| {
        ch.is_control() || ch.is_whitespace() || matches!(ch, '/' | '@' | '\\' | '?' | '#')
    }) {
        return Err(ParseError::new("HostPort", "bad_char"));
    }
    let (host, port) = split_host_port(raw)?;
    check_host(host)?;
    if let Some(port_text) = port {
        check_port(port_text)?;
    }
    Ok(HostPort(raw.to_owned()))
}

/// 主机与可选端口。
type SplitHostPort<'input> = (&'input str, Option<&'input str>);

/// 从右侧拆出可选端口。
///
/// # Errors
///
/// 空主机时返回错误。
fn split_host_port(raw: &str) -> Result<SplitHostPort<'_>, ParseError> {
    match raw.rsplit_once(':') {
        Some((host, port))
            if !port.is_empty() && port.bytes().all(|byte| byte.is_ascii_digit()) =>
        {
            if host.is_empty() {
                Err(ParseError::new("HostPort", "bad_host"))
            } else {
                Ok((host, Some(port)))
            }
        }
        Some(_) | None => Ok((raw, None)),
    }
}

/// 校验主机为 IPv4 或 FQDN。
///
/// # Errors
///
/// 两者都不是时返回错误。
fn check_host(host: &str) -> Result<(), ParseError> {
    if host.parse::<Ipv4Addr>().is_ok() {
        return Ok(());
    }
    Fqdn::parse(host)
        .map(|_| ())
        .map_err(|_error| ParseError::new("HostPort", "bad_host"))
}

/// 校验端口范围。
///
/// # Errors
///
/// 无法解析或不在 1–65535 时返回错误。
fn check_port(port_text: &str) -> Result<(), ParseError> {
    let port: u16 = port_text
        .parse()
        .map_err(|_error| ParseError::new("HostPort", "bad_port"))?;
    if port == 0 {
        Err(ParseError::new("HostPort", "bad_port"))
    } else {
        Ok(())
    }
}

/// NTP 服务器：IPv4 或 FQDN。
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct NtpServer(String);

impl NtpServer {
    /// 解析 NTP 主机。
    ///
    /// # Errors
    ///
    /// 既不是 IPv4 也不是 FQDN 时返回错误。
    pub fn parse(raw: &str) -> Result<Self, ParseError> {
        if raw.parse::<Ipv4Addr>().is_ok() {
            return Ok(Self(raw.to_owned()));
        }
        let fqdn = Fqdn::parse(raw).map_err(|_error| ParseError::new("NtpServer", "bad_host"))?;
        Ok(Self(fqdn.as_str().to_owned()))
    }

    /// 规范文本。
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for NtpServer {
    type Error = ParseError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

impl From<NtpServer> for String {
    fn from(value: NtpServer) -> Self {
        value.0
    }
}

impl Display for NtpServer {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// 以 `/` 开头的 Unix 绝对路径，不含 `..` 与空段。
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct UnixAbsPath(String);

impl UnixAbsPath {
    /// 解析绝对路径。
    ///
    /// # Errors
    ///
    /// 不以 `/` 开头、含控制字符、`..` 或空段、过长时返回错误。
    pub fn parse(raw: &str) -> Result<Self, ParseError> {
        parse_unix_abs(raw)
    }

    /// 路径文本。
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for UnixAbsPath {
    type Error = ParseError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

impl From<UnixAbsPath> for String {
    fn from(value: UnixAbsPath) -> Self {
        value.0
    }
}

impl Display for UnixAbsPath {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// 解析绝对路径主体。
///
/// # Errors
///
/// 格式不合法时返回错误。
fn parse_unix_abs(raw: &str) -> Result<UnixAbsPath, ParseError> {
    if raw.is_empty() {
        return Err(ParseError::new("UnixAbsPath", "empty"));
    }
    if raw.len() > 1024 {
        return Err(ParseError::new("UnixAbsPath", "too_long"));
    }
    if !raw.starts_with('/') {
        return Err(ParseError::new("UnixAbsPath", "not_absolute"));
    }
    if raw.chars().any(char::is_control) {
        return Err(ParseError::new("UnixAbsPath", "control_char"));
    }
    if raw.contains("//") {
        return Err(ParseError::new("UnixAbsPath", "empty_segment"));
    }
    let rest = raw
        .get(1..)
        .ok_or(ParseError::new("UnixAbsPath", "not_absolute"))?;
    if rest.split('/').any(|seg| seg == "." || seg == "..") {
        return Err(ParseError::new("UnixAbsPath", "dot_segment"));
    }
    Ok(UnixAbsPath(raw.to_owned()))
}

/// 镜像协议。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MirrorScheme {
    /// HTTP。
    Http,
    /// HTTPS。
    Https,
}

impl MirrorScheme {
    /// 协议名。
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Http => "http",
            Self::Https => "https",
        }
    }
}

impl Display for MirrorScheme {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Debian 镜像源。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mirror {
    /// 协议。
    pub scheme: MirrorScheme,
    /// 主机与可选端口。
    pub host_port: HostPort,
    /// 目录，例如 `/debian`。
    pub directory: UnixAbsPath,
    /// 可选 HTTP 代理。
    #[serde(default)]
    pub proxy: Option<HttpUrl>,
}

/// 内核 `console=` 参数，例如 `tty0` 或 `ttyS0,115200n8`。
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ConsoleSpec(String);

impl ConsoleSpec {
    /// 解析控制台规格。
    ///
    /// # Errors
    ///
    /// 格式不符或含非法字符时返回错误。
    pub fn parse(raw: &str) -> Result<Self, ParseError> {
        if raw.is_empty() {
            return Err(ParseError::new("ConsoleSpec", "empty"));
        }
        if raw.len() > 64 {
            return Err(ParseError::new("ConsoleSpec", "too_long"));
        }
        if is_console_spec(raw) {
            Ok(Self(raw.to_owned()))
        } else {
            Err(ParseError::new("ConsoleSpec", "bad_format"))
        }
    }

    /// 规格文本。
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// 仅用于已知合法的默认值。
    fn from_verified(raw: &str) -> Self {
        Self(raw.to_owned())
    }
}

impl TryFrom<String> for ConsoleSpec {
    type Error = ParseError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

impl From<ConsoleSpec> for String {
    fn from(value: ConsoleSpec) -> Self {
        value.0
    }
}

impl Display for ConsoleSpec {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// 是否为合法 `console=` 值。
fn is_console_spec(raw: &str) -> bool {
    let mut parts = raw.split(',');
    let Some(device) = parts.next() else {
        return false;
    };
    if !is_console_device(device) {
        return false;
    }
    parts
        .next()
        .is_none_or(|settings| is_console_settings(settings) && parts.next().is_none())
}

/// 设备名：`tty0`、`ttyS0`、`hvc0` 等。
fn is_console_device(raw: &str) -> bool {
    let bytes = raw.as_bytes();
    let Some((first, rest)) = bytes.split_first() else {
        return false;
    };
    first.is_ascii_lowercase() && rest.iter().all(u8::is_ascii_alphanumeric)
}

/// 波特率与校验，例如 `115200n8`。
fn is_console_settings(raw: &str) -> bool {
    let bytes = raw.as_bytes();
    !bytes.is_empty() && bytes.iter().all(u8::is_ascii_alphanumeric)
}

/// 有序控制台列表；默认 `tty0`。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsoleSet {
    /// 控制台条目。
    #[serde(default = "default_consoles")]
    pub items: Vec<ConsoleSpec>,
}

impl Default for ConsoleSet {
    fn default() -> Self {
        Self {
            items: default_consoles(),
        }
    }
}

impl ConsoleSet {
    /// 条目。
    #[must_use]
    pub fn items(&self) -> &[ConsoleSpec] {
        &self.items
    }
}

/// 默认控制台。
fn default_consoles() -> Vec<ConsoleSpec> {
    vec![ConsoleSpec::from_verified("tty0")]
}
