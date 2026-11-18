//! Linux bzImage 头部（boot protocol ≥ 2.00）中的内核版本串与 ABI。

use crate::error::{AssetsError, AssetsResult, fail};

/// `HdrS` 魔数偏移。
const HDRS_OFFSET: usize = 0x202;
/// `kernel_version` 指针偏移。
const VERSION_PTR_OFFSET: usize = 0x20E;
/// 版本串最长读取字节数。
const MAX_VERSION_LEN: usize = 256;

/// 内核版本信息。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KernelVersion {
    /// 完整版本串，例如 `6.12.107+deb13-amd64 (debian-kernel@…) #1 SMP …`。
    pub full: String,
    /// ABI（版本串第一个字段），例如 `6.12.107+deb13-amd64`。
    pub abi: String,
}

/// 从 bzImage 头部读取版本串。只需文件前若干 KiB。
///
/// # Errors
///
/// 不是 bzImage（`not_bzimage`）或版本指针越界/版本串不合法（`bad_kernel`）时返回错误。
pub fn kernel_version(head: &[u8]) -> AssetsResult<KernelVersion> {
    let magic_end = HDRS_OFFSET.saturating_add(4);
    if head.get(HDRS_OFFSET..magic_end) != Some(b"HdrS".as_slice()) {
        return Err(fail("not_bzimage", "缺少 HdrS 魔数，不是 bzImage"));
    }
    let ptr_end = VERSION_PTR_OFFSET.saturating_add(2);
    let ptr_bytes: [u8; 2] = head
        .get(VERSION_PTR_OFFSET..ptr_end)
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or_else(|| fail("bad_kernel", "头部截断"))?;
    let pointer = usize::from(u16::from_le_bytes(ptr_bytes));
    if pointer == 0 {
        return Err(fail("bad_kernel", "kernel_version 指针为 0"));
    }
    let start = pointer
        .checked_add(0x200)
        .ok_or_else(|| fail("bad_kernel", "版本指针溢出"))?;
    let window = head
        .get(start..)
        .filter(|rest| !rest.is_empty())
        .ok_or_else(|| {
            AssetsError::new(
                "bad_kernel",
                format!("版本指针 0x{start:x} 超出头部（{} 字节）", head.len()),
            )
        })?;
    let limit = window.len().min(MAX_VERSION_LEN);
    let bounded = window.get(..limit).unwrap_or_default();
    let end = bounded
        .iter()
        .position(|byte| *byte == 0)
        .ok_or_else(|| fail("bad_kernel", "版本串没有终止符"))?;
    let raw = bounded.get(..end).unwrap_or_default();
    let full = std::str::from_utf8(raw)
        .map_err(|_error| fail("bad_kernel", "版本串不是 UTF-8"))?
        .to_owned();
    let abi = full
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_owned();
    let valid = abi.starts_with(|ch: char| ch.is_ascii_digit())
        && abi
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b".+-_~".contains(&byte));
    if !valid {
        return Err(fail("bad_kernel", "版本串不像内核 ABI"));
    }
    Ok(KernelVersion { full, abi })
}

#[cfg(test)]
mod tests;
