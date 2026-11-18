//! 会话引导 ISO：`esp.img`（FAT，内含 `EFI/BOOT/BOOTX64.EFI` 与 `EFI/BOOT/autoexec.ipxe`）作为唯一的
//! UEFI El Torito 引导项。同一输入（会话 ID、创建时间、iPXE、脚本）得到逐字节相同的 ISO。

use crate::fat::{self, FileSpec};
use crate::iso::{self, Entry};
use crate::{ImageError, Timestamp};

/// ISO 内 ESP 映像的文件名（ISO9660 一级文件名只允许大写）。
pub const ESP_NAME: &str = "ESP.IMG";
/// ESP 内 iPXE 的路径。
pub const EFI_PATH: &str = "EFI/BOOT/BOOTX64.EFI";
/// ESP 内脚本的路径（iPXE `efi_autoexec` 先读加载目录）。
pub const AUTOEXEC_PATH: &str = "EFI/BOOT/autoexec.ipxe";

/// 生成会话引导 ISO 的输入。
#[derive(Debug, Clone, Copy)]
pub struct BootIsoInput<'data> {
    /// 会话 ID（取前 8 个字母数字作为卷标后缀）。
    pub session_id: &'data str,
    /// 会话创建时间：ISO 与 FAT 内所有时间戳都取它。
    pub created: Timestamp,
    /// 本会话固定的 `ipxe.efi` 字节。
    pub ipxe_efi: &'data [u8],
    /// 渲染好的 `autoexec.ipxe`。
    pub autoexec: &'data [u8],
}

/// 卷标 `XPXE-<会话前 8 位>`（大写字母数字）。
///
/// # Errors
///
/// 会话 ID 中字母数字不足 8 个时返回 [`ImageError::BadPath`]。
pub fn volume_label(session_id: &str) -> Result<String, ImageError> {
    let short: String = session_id
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .take(8)
        .map(|ch| ch.to_ascii_uppercase())
        .collect();
    if short.len() < 8 {
        return Err(ImageError::BadPath(format!(
            "会话 ID 至少需要 8 个字母数字：{session_id}"
        )));
    }
    Ok(format!("XPXE-{short}"))
}

/// 生成会话引导 ISO。
///
/// # Errors
///
/// 会话 ID 不合法、时间戳越界或内容超出格式上限时返回错误。
pub fn build_boot_iso(input: &BootIsoInput<'_>) -> Result<Vec<u8>, ImageError> {
    let label = volume_label(input.session_id)?;
    let esp = fat::build(
        &[
            FileSpec {
                path: EFI_PATH,
                data: input.ipxe_efi,
            },
            FileSpec {
                path: AUTOEXEC_PATH,
                data: input.autoexec,
            },
        ],
        "XPXE-ESP",
        input.created,
    )?;
    iso::build(
        &[Entry {
            name: ESP_NAME,
            data: &esp.bytes,
        }],
        ESP_NAME,
        &label,
        input.created,
    )
}

#[cfg(test)]
mod tests;
