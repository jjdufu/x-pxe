//! APT 组件、套件、安全源与额外仓库。

use serde::{Deserialize, Serialize};

use super::host::{HostPort, UnixAbsPath};
use crate::value::{HttpUrl, ParseError, SafeText};

/// APT 组件。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AptComponent {
    /// `main`。
    Main,
    /// `contrib`。
    Contrib,
    /// `non-free`。
    NonFree,
    /// `non-free-firmware`。
    NonFreeFirmware,
}

impl AptComponent {
    /// 官方组件名。
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Main => "main",
            Self::Contrib => "contrib",
            Self::NonFree => "non-free",
            Self::NonFreeFirmware => "non-free-firmware",
        }
    }
}

/// APT 组件集合；默认仅 `main`。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComponentSet {
    /// 组件列表（去重、有序由调用方保证或在访问时去重）。
    #[serde(default = "default_components")]
    pub items: Vec<AptComponent>,
}

impl Default for ComponentSet {
    fn default() -> Self {
        Self {
            items: default_components(),
        }
    }
}

impl ComponentSet {
    /// 去重后的组件（`main` 始终保留）。
    #[must_use]
    pub fn unique(&self) -> Vec<AptComponent> {
        unique_components(&self.items)
    }

    /// 是否包含指定组件。
    #[must_use]
    pub fn contains(&self, component: AptComponent) -> bool {
        self.unique().contains(&component)
    }
}

/// 默认组件。
fn default_components() -> Vec<AptComponent> {
    vec![AptComponent::Main]
}

/// 保证含 `main` 并去重、按枚举序排列。
fn unique_components(items: &[AptComponent]) -> Vec<AptComponent> {
    let mut seen = [
        AptComponent::Main,
        AptComponent::Contrib,
        AptComponent::NonFree,
        AptComponent::NonFreeFirmware,
    ]
    .map(|_| false);
    if let Some(slot) = seen.get_mut(0) {
        *slot = true;
    }
    for item in items {
        if let Some(slot) = seen.get_mut(component_index(*item)) {
            *slot = true;
        }
    }
    [
        AptComponent::Main,
        AptComponent::Contrib,
        AptComponent::NonFree,
        AptComponent::NonFreeFirmware,
    ]
    .into_iter()
    .zip(seen)
    .filter_map(|(component, keep)| keep.then_some(component))
    .collect()
}

/// 组件在固定表中的下标。
const fn component_index(component: AptComponent) -> usize {
    match component {
        AptComponent::Main => 0,
        AptComponent::Contrib => 1,
        AptComponent::NonFree => 2,
        AptComponent::NonFreeFirmware => 3,
    }
}

/// updates / backports 开关。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub struct SuiteSet {
    /// 是否启用 `trixie-updates`。
    #[serde(default)]
    pub updates: bool,
    /// 是否启用 `trixie-backports`。
    #[serde(default)]
    pub backports: bool,
}

/// 安全更新源。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SecuritySource {
    /// 官方 `security.debian.org`。
    Official,
    /// 自定义安全镜像。
    Mirror {
        /// 主机与可选端口。
        host_port: HostPort,
        /// 目录。
        directory: UnixAbsPath,
    },
    /// 不配置安全更新。
    Disabled,
}

/// ASCII-armored `OpenPGP` 公钥（结构校验；完整解析见 `xpxe-assets`）。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ArmoredKey(String);

impl ArmoredKey {
    /// 解析装甲块。
    ///
    /// # Errors
    ///
    /// 不是公钥装甲块或含控制字符时返回错误。
    pub fn parse(raw: &str) -> Result<Self, ParseError> {
        parse_armored(raw)
    }

    /// 装甲原文。
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for ArmoredKey {
    type Error = ParseError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

impl From<ArmoredKey> for String {
    fn from(value: ArmoredKey) -> Self {
        value.0
    }
}

/// 校验装甲公钥外形。
///
/// # Errors
///
/// 外形不合法时返回错误。
fn parse_armored(raw: &str) -> Result<ArmoredKey, ParseError> {
    const BEGIN: &str = "-----BEGIN PGP PUBLIC KEY BLOCK-----";
    const END: &str = "-----END PGP PUBLIC KEY BLOCK-----";
    let trimmed = raw.trim();
    if trimmed.len() > 65_536 {
        return Err(ParseError::new("ArmoredKey", "too_long"));
    }
    if trimmed
        .chars()
        .any(|ch| ch.is_control() && ch != '\n' && ch != '\r')
    {
        return Err(ParseError::new("ArmoredKey", "control_char"));
    }
    if !trimmed.starts_with(BEGIN) || !trimmed.ends_with(END) {
        return Err(ParseError::new("ArmoredKey", "bad_armor"));
    }
    Ok(ArmoredKey(trimmed.to_owned()))
}

/// 额外 APT 仓库。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AptRepo {
    /// 仓库 URL。
    pub url: HttpUrl,
    /// suite 名，例如 `trixie`。
    pub suite: SafeText,
    /// 组件。
    pub components: ComponentSet,
    /// 仓库公钥。
    pub key: ArmoredKey,
}
