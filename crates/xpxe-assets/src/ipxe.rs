//! iPXE 二进制格式检查（官方预编译文件不带签名，只做结构检查）。

use serde::Serialize;

use crate::error::{AssetsError, AssetsResult, fail};

/// `e_lfanew` 偏移。
const LFANEW_OFFSET: usize = 0x3C;
/// x86-64。
const MACHINE_AMD64: u16 = 0x8664;
/// PE32+ Optional Header Magic。
const PE32_PLUS: u16 = 0x020B;
/// `EFI_APPLICATION`。
const SUBSYSTEM_EFI_APPLICATION: u16 = 10;
/// `undionly.kpxe` 大小下限。
const KPXE_MIN: usize = 32 * 1024;
/// `undionly.kpxe` 大小上限。
const KPXE_MAX: usize = 512 * 1024;

/// 格式检查结论。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IpxeFormat {
    /// 格式说明，例如 `PE32+ x86-64 EFI application`。
    pub format: &'static str,
    /// 二进制中可见的 iPXE 版本串（官方构建经压缩时可能为空）。
    pub version: Option<String>,
}

/// 检查 x86-64 EFI 应用（`ipxe.efi`、`snponly.efi`）。
///
/// # Errors
///
/// 不是 PE32+、Machine 不是 0x8664 或 Subsystem 不是 `EFI_APPLICATION` 时返回 `bad_format`。
pub fn check_efi_x64(data: &[u8]) -> AssetsResult<IpxeFormat> {
    if data.get(..2) != Some(b"MZ".as_slice()) {
        return Err(fail("bad_format", "缺少 MZ 头"));
    }
    let lfanew = usize::try_from(read_u32(data, LFANEW_OFFSET)?)
        .map_err(|_error| fail("bad_format", "e_lfanew 越界"))?;
    let pe_end = lfanew.saturating_add(4);
    if data.get(lfanew..pe_end) != Some(b"PE\0\0".as_slice()) {
        return Err(fail("bad_format", "缺少 PE 签名"));
    }
    let machine = read_u16(data, pe_end)?;
    if machine != MACHINE_AMD64 {
        return Err(AssetsError::new(
            "bad_format",
            format!("Machine 为 0x{machine:04x}，需要 x86-64（0x8664）"),
        ));
    }
    let optional = pe_end.saturating_add(20);
    let magic = read_u16(data, optional)?;
    if magic != PE32_PLUS {
        return Err(AssetsError::new(
            "bad_format",
            format!("Optional Header Magic 为 0x{magic:04x}，需要 PE32+（0x20b）"),
        ));
    }
    let subsystem = read_u16(data, optional.saturating_add(68))?;
    if subsystem != SUBSYSTEM_EFI_APPLICATION {
        return Err(AssetsError::new(
            "bad_format",
            format!("Subsystem 为 {subsystem}，需要 EFI_APPLICATION（10）"),
        ));
    }
    check_sections(data, pe_end, optional)?;
    Ok(IpxeFormat {
        format: "PE32+ x86-64 EFI application",
        version: find_version(data),
    })
}

/// 节表中每个节的原始数据都必须完整落在文件内（拒绝截断文件）。
///
/// # Errors
///
/// 节表越界或节数据超出文件末尾时返回 `bad_format`。
fn check_sections(data: &[u8], coff: usize, optional: usize) -> AssetsResult<()> {
    let count = usize::from(read_u16(data, coff.saturating_add(2))?);
    let optional_size = usize::from(read_u16(data, coff.saturating_add(16))?);
    let table = optional.saturating_add(optional_size);
    if count == 0 {
        return Err(fail("bad_format", "没有节"));
    }
    for index in 0..count {
        let entry = table.saturating_add(index.saturating_mul(40));
        let raw_size =
            usize::try_from(read_u32(data, entry.saturating_add(16))?).unwrap_or(usize::MAX);
        let raw_ptr =
            usize::try_from(read_u32(data, entry.saturating_add(20))?).unwrap_or(usize::MAX);
        if raw_ptr.saturating_add(raw_size) > data.len() {
            return Err(AssetsError::new(
                "bad_format",
                format!("文件截断：第 {index} 节超出文件末尾（{} 字节）", data.len()),
            ));
        }
    }
    Ok(())
}

/// 检查 BIOS PXE 用的 `undionly.kpxe`：大小 32–512 KiB，以 iPXE PXE 前缀的
/// `ljmp $0x07c0, …` 开头。官方构建为压缩映像，不包含明文 `iPXE` 版本串。
///
/// # Errors
///
/// 大小或前缀不符时返回 `bad_format`。
pub fn check_undionly(data: &[u8]) -> AssetsResult<IpxeFormat> {
    if !(KPXE_MIN..=KPXE_MAX).contains(&data.len()) {
        return Err(AssetsError::new(
            "bad_format",
            format!("大小 {} 字节不在 32 KiB–512 KiB 之间", data.len()),
        ));
    }
    let prefix_ok = data.first() == Some(&0xEA) && data.get(3..5) == Some([0xC0, 0x07].as_slice());
    if !prefix_ok {
        return Err(fail("bad_format", "不是 iPXE PXE 前缀（缺少 ljmp 0x07c0）"));
    }
    Ok(IpxeFormat {
        format: "iPXE PXE NBP (undionly.kpxe)",
        version: find_version(data),
    })
}

/// 搜索明文 `iPXE x.y.z…` 版本串。
fn find_version(data: &[u8]) -> Option<String> {
    let needle = b"iPXE ";
    data.windows(needle.len())
        .enumerate()
        .filter(|(_, window)| *window == needle)
        .find_map(|(index, _)| {
            let start = index.saturating_add(needle.len());
            let tail = data.get(start..)?;
            let len = tail
                .iter()
                .take(64)
                .position(|byte| !(byte.is_ascii_graphic() || *byte == b' '))
                .unwrap_or_else(|| tail.len().min(64));
            let text = std::str::from_utf8(tail.get(..len)?).ok()?;
            text.starts_with(|ch: char| ch.is_ascii_digit())
                .then(|| format!("iPXE {}", text.trim_end()))
        })
}

/// 读取小端 u16。
///
/// # Errors
///
/// 越界时返回 `bad_format`。
fn read_u16(data: &[u8], offset: usize) -> AssetsResult<u16> {
    data.get(offset..offset.saturating_add(2))
        .and_then(|bytes| bytes.try_into().ok())
        .map(u16::from_le_bytes)
        .ok_or_else(|| fail("bad_format", "文件截断"))
}

/// 读取小端 u32。
///
/// # Errors
///
/// 越界时返回 `bad_format`。
fn read_u32(data: &[u8], offset: usize) -> AssetsResult<u32> {
    data.get(offset..offset.saturating_add(4))
        .and_then(|bytes| bytes.try_into().ok())
        .map(u32::from_le_bytes)
        .ok_or_else(|| fail("bad_format", "文件截断"))
}

#[cfg(test)]
mod tests;
