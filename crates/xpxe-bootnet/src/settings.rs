//! 环境变量：`XPXE_CONTROL_URL`、`XPXE_BOOTNET_TOKEN_FILE`、`XPXE_BOOTNET_ALLOWED_IFACES`。

use std::fmt::{Debug, Formatter};

use xpxe_domain::NetIface;

use crate::BootnetError;

/// 默认控制面内部地址。
const DEFAULT_CONTROL_URL: &str = "http://127.0.0.1:8081";
/// 默认令牌文件（Docker secret）。
const DEFAULT_TOKEN_FILE: &str = "/run/secrets/xpxe_bootnet_token";

/// Bearer 令牌；`Debug` 不输出内容。
#[derive(Clone, PartialEq, Eq)]
pub struct Token(String);

impl Token {
    /// 由文本构造（去掉首尾空白）。
    ///
    /// # Errors
    ///
    /// 为空或含空白/控制字符时返回错误。
    pub fn parse(raw: &str) -> Result<Self, BootnetError> {
        let trimmed = raw.trim();
        if trimmed.is_empty()
            || trimmed
                .chars()
                .any(|ch| ch.is_whitespace() || ch.is_control())
        {
            return Err(BootnetError::new("config", "bootnet 令牌为空或含空白字符"));
        }
        Ok(Self(trimmed.to_owned()))
    }

    /// 令牌文本（只用于拼 `Authorization` 头）。
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl Debug for Token {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str("Token(***)")
    }
}

/// 进程配置。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// 控制面内部 API 基址，例如 `http://127.0.0.1:8081`（只支持 `http`）。
    pub control_url: String,
    /// Bearer 令牌。
    pub token: Token,
    /// 允许监听的网卡。
    pub allowed_ifaces: Vec<NetIface>,
}

impl Settings {
    /// 从进程环境读取。
    ///
    /// # Errors
    ///
    /// 变量缺失、格式错误或令牌文件不可读时返回错误。
    pub fn from_env() -> Result<Self, BootnetError> {
        Self::from_lookup(|key| std::env::var(key).ok())
    }

    /// 从任意查找函数读取（便于测试）。
    ///
    /// # Errors
    ///
    /// 同 [`Self::from_env`]。
    pub fn from_lookup<F: Fn(&str) -> Option<String>>(lookup: F) -> Result<Self, BootnetError> {
        let raw_url = lookup("XPXE_CONTROL_URL").unwrap_or_else(|| DEFAULT_CONTROL_URL.to_owned());
        let control_url = parse_control_url(&raw_url)?;
        let token_file =
            lookup("XPXE_BOOTNET_TOKEN_FILE").unwrap_or_else(|| DEFAULT_TOKEN_FILE.to_owned());
        let raw = std::fs::read_to_string(&token_file).map_err(|error| {
            BootnetError::new("config", format!("读取令牌文件 {token_file} 失败：{error}"))
        })?;
        let token = Token::parse(&raw)?;
        let ifaces = lookup("XPXE_BOOTNET_ALLOWED_IFACES")
            .ok_or_else(|| BootnetError::new("config", "必须设置 XPXE_BOOTNET_ALLOWED_IFACES"))?;
        let allowed_ifaces = parse_ifaces(&ifaces)?;
        Ok(Self {
            control_url,
            token,
            allowed_ifaces,
        })
    }
}

/// 校验控制面 URL：`http://host:port`，去掉末尾 `/`。
///
/// # Errors
///
/// 不是 `http://` 或缺少主机时返回错误。
fn parse_control_url(raw: &str) -> Result<String, BootnetError> {
    let trimmed = raw.trim().trim_end_matches('/');
    let rest = trimmed.strip_prefix("http://").ok_or_else(|| {
        BootnetError::new("config", "XPXE_CONTROL_URL 只支持 http://（内部回环地址）")
    })?;
    if rest.is_empty() || rest.contains('/') || rest.chars().any(char::is_whitespace) {
        return Err(BootnetError::new(
            "config",
            "XPXE_CONTROL_URL 须为 http://<主机>:<端口>",
        ));
    }
    Ok(trimmed.to_owned())
}

/// 解析逗号分隔的网卡白名单。
///
/// # Errors
///
/// 为空或含非法网卡名时返回错误。
fn parse_ifaces(raw: &str) -> Result<Vec<NetIface>, BootnetError> {
    let list = raw
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(|item| {
            NetIface::parse(item).map_err(|error| {
                BootnetError::new("config", format!("网卡名 {item} 非法：{}", error.code()))
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    if list.is_empty() {
        return Err(BootnetError::new(
            "config",
            "XPXE_BOOTNET_ALLOWED_IFACES 为空",
        ));
    }
    Ok(list)
}

#[cfg(test)]
mod tests;
