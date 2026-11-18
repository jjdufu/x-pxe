//! 资产种类与资产频道配置文档（`asset_channels`）。

use std::collections::BTreeMap;
use std::fmt::{Display, Formatter};
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use xpxe_domain::{
    ConfigDocument, ConfigError, ConfigKind, FieldError, field_error, migrate_current,
};

use crate::error::{AssetsError, AssetsResult};
use crate::release::is_sha256_hex;

/// 1 MiB。
const MIB: u64 = 1024 * 1024;

/// 资产种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetKind {
    /// BIOS PXE 用 `undionly.kpxe`。
    IpxeUndionly,
    /// UEFI x86-64 `ipxe.efi`。
    IpxeEfiX64,
    /// UEFI x86-64 `snponly.efi`。
    IpxeSnponlyX64,
    /// Debian netboot 内核与 initrd（清单 + 两个成员文件）。
    DebianNetbootAmd64,
    /// Debian 非自由固件 `firmware.cpio.gz`。
    DebianFirmwareCpio,
}

/// 全部种类。
pub const ALL_KINDS: [AssetKind; 5] = [
    AssetKind::IpxeUndionly,
    AssetKind::IpxeEfiX64,
    AssetKind::IpxeSnponlyX64,
    AssetKind::DebianNetbootAmd64,
    AssetKind::DebianFirmwareCpio,
];

impl AssetKind {
    /// 稳定名称。
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::IpxeUndionly => "ipxe_undionly",
            Self::IpxeEfiX64 => "ipxe_efi_x64",
            Self::IpxeSnponlyX64 => "ipxe_snponly_x64",
            Self::DebianNetbootAmd64 => "debian_netboot_amd64",
            Self::DebianFirmwareCpio => "debian_firmware_cpio",
        }
    }

    /// 官方下载地址；netboot 取自 Profile 镜像源，固件为目录地址。
    ///
    /// boot.ipxe.org 自 iPXE 2.0 起把 EFI 构建移到 `x86_64-efi/` 子目录，根路径的
    /// `ipxe.efi` 已返回 404。
    #[must_use]
    pub const fn official_url(self) -> Option<&'static str> {
        match self {
            Self::IpxeUndionly => Some("https://boot.ipxe.org/undionly.kpxe"),
            Self::IpxeEfiX64 => Some("https://boot.ipxe.org/x86_64-efi/ipxe.efi"),
            Self::IpxeSnponlyX64 => Some("https://boot.ipxe.org/x86_64-efi/snponly.efi"),
            Self::DebianNetbootAmd64 => None,
            Self::DebianFirmwareCpio => {
                Some("https://cdimage.debian.org/cdimage/firmware/trixie/current/")
            }
        }
    }

    /// 单个文件的大小上限。
    #[must_use]
    pub const fn size_limit(self) -> u64 {
        match self {
            Self::IpxeUndionly | Self::IpxeEfiX64 | Self::IpxeSnponlyX64 => 4 * MIB,
            Self::DebianNetbootAmd64 => 256 * MIB,
            // trixie 的 firmware.cpio.gz 约 473 MiB（2026-11）
            Self::DebianFirmwareCpio => 1024 * MIB,
        }
    }

    /// TFTP/HTTP 下发时使用的文件名（netboot 为成员名之一，见清单）。
    #[must_use]
    pub const fn file_name(self) -> &'static str {
        match self {
            Self::IpxeUndionly => "undionly.kpxe",
            Self::IpxeEfiX64 => "ipxe.efi",
            Self::IpxeSnponlyX64 => "snponly.efi",
            Self::DebianNetbootAmd64 => "netboot.manifest",
            Self::DebianFirmwareCpio => "firmware.cpio.gz",
        }
    }

    /// 是否为 iPXE 固件。
    #[must_use]
    pub const fn is_ipxe(self) -> bool {
        matches!(
            self,
            Self::IpxeUndionly | Self::IpxeEfiX64 | Self::IpxeSnponlyX64
        )
    }
}

impl Display for AssetKind {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for AssetKind {
    type Err = AssetsError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        ALL_KINDS
            .into_iter()
            .find(|kind| kind.as_str() == value)
            .ok_or_else(|| AssetsError::new("unknown_kind", format!("未知资产种类 {value}")))
    }
}

/// 频道中一个种类当前激活的资产。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChannelEntry {
    /// 资产 ID。
    pub asset_id: i64,
    /// 资产 SHA-256（bootnet 据此生成 `fw/<前 12 位>/<文件名>`）。
    pub sha256: String,
}

/// 资产频道：每个种类当前激活的资产。激活与回滚都生成新版本。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssetChannels {
    /// 种类 → 激活资产。
    pub channels: BTreeMap<AssetKind, ChannelEntry>,
}

impl AssetChannels {
    /// 是否引用了某资产。
    #[must_use]
    pub fn references(&self, asset_id: i64) -> bool {
        self.channels
            .values()
            .any(|entry| entry.asset_id == asset_id)
    }
}

impl ConfigDocument for AssetChannels {
    const KIND: ConfigKind = ConfigKind::AssetChannels;
    const SCHEMA: u32 = 1;

    fn validate(&self) -> Result<(), Vec<FieldError>> {
        let mut errors = Vec::new();
        for (kind, entry) in &self.channels {
            let path = format!("/channels/{kind}");
            if entry.asset_id <= 0 {
                errors.push(field_error(
                    &format!("{path}/asset_id"),
                    "invalid",
                    "资产 ID 必须为正",
                ));
            }
            if !is_sha256_hex(&entry.sha256) {
                errors.push(field_error(
                    &format!("{path}/sha256"),
                    "invalid",
                    "SHA-256 必须为 64 位十六进制",
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
        migrate_current(from_schema, value)
    }
}

/// 解析种类名称。
///
/// # Errors
///
/// 未知名称时返回错误。
pub fn parse_kind(value: &str) -> AssetsResult<AssetKind> {
    value.parse()
}

#[cfg(test)]
mod tests;
