//! 确定性选盘规则。

use serde::{Deserialize, Serialize};

use crate::value::{ParseError, SafeText};

/// 磁盘传输类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiskTransport {
    /// `NVMe`。
    Nvme,
    /// SATA / ATA。
    Sata,
    /// SAS。
    Sas,
    /// virtio-blk / virtio-scsi。
    Virtio,
    /// 通用 SCSI。
    Scsi,
}

impl DiskTransport {
    /// 稳定名称。
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Nvme => "nvme",
            Self::Sata => "sata",
            Self::Sas => "sas",
            Self::Virtio => "virtio",
            Self::Scsi => "scsi",
        }
    }
}

/// 容量范围（GiB）。默认下限 16。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SizeRange {
    /// 下限（含），GiB。
    #[serde(default = "default_min_gib")]
    pub min: u64,
    /// 上限（含），缺省表示无上限。
    #[serde(default)]
    pub max: Option<u64>,
}

impl Default for SizeRange {
    fn default() -> Self {
        Self {
            min: default_min_gib(),
            max: None,
        }
    }
}

impl SizeRange {
    /// 范围是否合法（有上限时不低于下限）。
    #[must_use]
    pub const fn is_valid(self) -> bool {
        match self.max {
            Some(max) => max >= self.min,
            None => true,
        }
    }

    /// 容量（GiB）是否落在范围内。
    #[must_use]
    pub fn contains(self, gib: u64) -> bool {
        if gib < self.min {
            return false;
        }
        self.max.is_none_or(|max| gib <= max)
    }
}

/// 默认最小容量。
const fn default_min_gib() -> u64 {
    16
}

/// 多块候选盘时的选择策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Prefer {
    /// 必须恰好一块匹配。
    #[default]
    Unique,
    /// 选最小的一块。
    Smallest,
    /// 选最大的一块。
    Largest,
}

/// 选盘匹配条件。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DiskMatch {
    /// `/dev/disk/by-id` 通配（仅 `*` / `?`）。
    ById {
        /// 通配模式。
        glob: SafeText,
    },
    /// WWN / NAA。
    Wwn {
        /// WWN 文本。
        value: SafeText,
    },
    /// 序列号。
    Serial {
        /// 序列号。
        value: SafeText,
    },
    /// 型号正则（受限字符集）。
    Model {
        /// 正则。
        regex: SafeText,
    },
    /// 传输类型。
    Transport {
        /// 类型。
        kind: DiskTransport,
    },
}

impl DiskMatch {
    /// 校验通配/正则字符集。
    ///
    /// # Errors
    ///
    /// 模式含不允许的字符时返回错误。
    pub fn check(&self) -> Result<(), ParseError> {
        match self {
            Self::ById { glob } => check_glob(glob.as_str()),
            Self::Model { regex } => check_model_regex(regex.as_str()),
            Self::Wwn { .. } | Self::Serial { .. } | Self::Transport { .. } => Ok(()),
        }
    }
}

/// by-id 通配：字母数字、`._-*?`。
///
/// # Errors
///
/// 字符不合法时返回错误。
fn check_glob(raw: &str) -> Result<(), ParseError> {
    let ok = raw.bytes().all(|byte| {
        byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'*' | b'?')
    });
    if ok {
        Ok(())
    } else {
        Err(ParseError::new("DiskMatch", "bad_glob"))
    }
}

/// 型号正则：字母数字与少量元字符，禁止 `$`、反引号等。
///
/// # Errors
///
/// 字符不合法时返回错误。
fn check_model_regex(raw: &str) -> Result<(), ParseError> {
    let ok = raw.bytes().all(|byte| {
        byte.is_ascii_alphanumeric()
            || matches!(
                byte,
                b'.' | b'_' | b'-' | b'*' | b'?' | b'[' | b']' | b'^' | b'+'
            )
    });
    if ok {
        Ok(())
    } else {
        Err(ParseError::new("DiskMatch", "bad_regex"))
    }
}

/// 选盘规则。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiskRule {
    /// 正向匹配；空表示凡容量与排除规则通过的盘都可入选。
    #[serde(default)]
    pub include: Vec<DiskMatch>,
    /// 容量范围。
    #[serde(default)]
    pub size_gib: SizeRange,
    /// 按 vendor/model 子串排除；默认含 BMC 虚拟介质特征。
    #[serde(default = "default_exclude_models")]
    pub exclude_models: Vec<SafeText>,
    /// 多候选时的策略。
    #[serde(default)]
    pub prefer: Prefer,
}

impl Default for DiskRule {
    fn default() -> Self {
        Self {
            include: Vec::new(),
            size_gib: SizeRange::default(),
            exclude_models: default_exclude_models(),
            prefer: Prefer::Unique,
        }
    }
}

impl DiskRule {
    /// 校验范围与匹配模式。
    ///
    /// # Errors
    ///
    /// 范围颠倒或模式非法时返回错误码文本。
    pub fn check(&self) -> Result<(), ParseError> {
        if !self.size_gib.is_valid() {
            return Err(ParseError::new("SizeRange", "inverted"));
        }
        for rule in &self.include {
            rule.check()?;
        }
        Ok(())
    }
}

/// 默认排除的 BMC / 虚拟介质型号特征。
#[must_use]
pub fn default_exclude_models() -> Vec<SafeText> {
    const NAMES: &[&str] = &[
        "Virtual",
        "iDRAC",
        "iLO",
        "AMI",
        "Cypress",
        "Linux File-Stor",
        "RMM",
        "Virtual CDROM",
        "Virtual Floppy",
    ];
    NAMES
        .iter()
        .filter_map(|name| SafeText::parse(name).ok())
        .collect()
}
