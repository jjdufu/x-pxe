//! 安装期账户与口令哈希。

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::value::{ParseError, SafeText, UnixUser};
use crate::versioning::{FieldError, field_error};

/// Unix 组名：与用户名相同的字符规则，不套用保留用户名单。
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct UnixGroup(String);

impl UnixGroup {
    /// 解析组名。
    ///
    /// # Errors
    ///
    /// 格式不符时返回错误。
    pub fn parse(raw: &str) -> Result<Self, ParseError> {
        if raw.is_empty() {
            return Err(ParseError::new("UnixGroup", "empty"));
        }
        if raw.len() > 32 {
            return Err(ParseError::new("UnixGroup", "too_long"));
        }
        if is_group_name(raw) {
            Ok(Self(raw.to_owned()))
        } else {
            Err(ParseError::new("UnixGroup", "bad_format"))
        }
    }

    /// 组名文本。
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for UnixGroup {
    type Error = ParseError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

impl From<UnixGroup> for String {
    fn from(value: UnixGroup) -> Self {
        value.0
    }
}

/// 组名字符规则。
fn is_group_name(raw: &str) -> bool {
    let bytes = raw.as_bytes();
    let Some((first, rest)) = bytes.split_first() else {
        return false;
    };
    let first_ok = first.is_ascii_lowercase() || *first == b'_';
    first_ok
        && rest.iter().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_' || *byte == b'-'
        })
}

/// `$6$`（sha-crypt）或 `$y$`（yescrypt）口令哈希。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CryptHash(String);

impl CryptHash {
    /// 解析哈希。
    ///
    /// # Errors
    ///
    /// 前缀或字段格式不合法时返回错误。
    pub fn parse(raw: &str) -> Result<Self, ParseError> {
        if is_sha512_crypt(raw) || is_yescrypt(raw) {
            Ok(Self(raw.to_owned()))
        } else {
            Err(ParseError::new("CryptHash", "bad_format"))
        }
    }

    /// 哈希文本。
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// 计算哈希的 SHA-256 十六进制前 8 位。
#[must_use]
pub fn hash_fingerprint(hash: &CryptHash) -> String {
    let hex = hex::encode(Sha256::digest(hash.as_str().as_bytes()));
    hex.get(..8).map_or_else(String::new, ToOwned::to_owned)
}

/// 是否为合法 `$6$` sha512crypt。
fn is_sha512_crypt(raw: &str) -> bool {
    let Some(rest) = raw.strip_prefix("$6$") else {
        return false;
    };
    rest.split_once('$').is_some_and(|(first, tail)| {
        first.strip_prefix("rounds=").map_or_else(
            || is_crypt_salt(first) && is_sha512_digest(tail),
            |rounds| is_rounds(rounds) && is_salt_and_digest(tail),
        )
    })
}

/// `rounds=` 数字是否在 sha-crypt 允许范围内。
fn is_rounds(raw: &str) -> bool {
    raw.parse::<u32>()
        .is_ok_and(|value| (1000..=999_999_999).contains(&value))
}

/// `salt$hash` 两段。
fn is_salt_and_digest(raw: &str) -> bool {
    match raw.split_once('$') {
        Some((salt, digest)) => is_crypt_salt(salt) && is_sha512_digest(digest),
        None => false,
    }
}

/// sha512crypt 盐：1–16 位 `[A-Za-z0-9./]`。
fn is_crypt_salt(raw: &str) -> bool {
    let len = raw.len();
    (1..=16).contains(&len) && raw.bytes().all(is_crypt_alphabet)
}

/// sha512crypt 摘要：86 位 `[A-Za-z0-9./]`。
fn is_sha512_digest(raw: &str) -> bool {
    raw.len() == 86 && raw.bytes().all(is_crypt_alphabet)
}

/// crypt 字母表。
const fn is_crypt_alphabet(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'/'
}

/// 是否为合法 `$y$` yescrypt（` $y$params$salt$hash `）。
fn is_yescrypt(raw: &str) -> bool {
    let Some(rest) = raw.strip_prefix("$y$") else {
        return false;
    };
    let parts: Vec<&str> = rest.split('$').collect();
    parts.len() == 3
        && parts.iter().all(|part| {
            !part.is_empty() && part.len() <= 256 && part.bytes().all(is_crypt_alphabet)
        })
}

/// 写入配置文档的哈希：序列化带指纹，差异中只显示“已更改”。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CryptSecret {
    /// 引用标签。
    secret_ref: String,
    /// SHA-256 十六进制前 8 位。
    fingerprint: String,
    /// 完整哈希。
    hash: String,
}

impl CryptSecret {
    /// 由已校验哈希构造。
    #[must_use]
    pub fn from_hash(hash: &CryptHash) -> Self {
        let fingerprint = hash_fingerprint(hash);
        Self {
            secret_ref: "crypt_hash".to_owned(),
            fingerprint,
            hash: hash.as_str().to_owned(),
        }
    }

    /// 解析并包装。
    ///
    /// # Errors
    ///
    /// 哈希格式不合法时返回错误。
    pub fn parse(raw: &str) -> Result<Self, ParseError> {
        Ok(Self::from_hash(&CryptHash::parse(raw)?))
    }

    /// 已校验的哈希。
    ///
    /// # Errors
    ///
    /// 存储内容损坏时返回错误。
    pub fn crypt_hash(&self) -> Result<CryptHash, ParseError> {
        CryptHash::parse(&self.hash)
    }

    /// 哈希文本。
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.hash
    }

    /// 指纹。
    #[must_use]
    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }
}

/// 根账户策略。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RootAccount {
    /// 禁止 root 登录，必须另有带 `sudo` 的主用户。
    Disabled,
    /// 使用给定哈希启用 root。
    Hash {
        /// 口令哈希。
        hash: CryptSecret,
    },
}

/// 安装期主用户。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrimaryUser {
    /// 登录名。
    pub name: UnixUser,
    /// 全名。
    pub full_name: SafeText,
    /// 口令哈希。
    pub hash: CryptSecret,
    /// 附加组。
    #[serde(default)]
    pub groups: Vec<UnixGroup>,
}

impl PrimaryUser {
    /// 是否包含 `sudo` 组。
    #[must_use]
    pub fn has_sudo(&self) -> bool {
        self.groups.iter().any(|group| group.as_str() == "sudo")
    }
}

/// 安装账户。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallAccounts {
    /// 根账户。
    pub root: RootAccount,
    /// 可选主用户。
    #[serde(default)]
    pub primary_user: Option<PrimaryUser>,
}

impl InstallAccounts {
    /// 收集跨字段错误。
    pub fn collect_errors(&self, errors: &mut Vec<FieldError>) {
        if matches!(self.root, RootAccount::Disabled) {
            match &self.primary_user {
                None => errors.push(field_error(
                    "/accounts/primary_user",
                    "required_when_root_disabled",
                    "禁用 root 时必须设置带 sudo 组的主用户",
                )),
                Some(user) if !user.has_sudo() => errors.push(field_error(
                    "/accounts/primary_user/groups",
                    "sudo_required",
                    "禁用 root 时主用户必须属于 sudo 组",
                )),
                Some(_) => {}
            }
        }
    }
}
