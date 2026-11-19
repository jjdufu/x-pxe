//! newc（SVR4，无校验和）cpio 写入器：内核 initramfs 支持把多个 cpio 段串联在压缩 initrd 之后。
//!
//! 只写普通文件与结束标记 `TRAILER!!!`；文件内容流式拷贝，不在内存中缓存。

use std::io::{Read, Write};

use crate::ImageError;

/// newc 魔数（无校验和）。
const MAGIC: &str = "070701";
/// 普通文件、权限 0644。
const MODE_FILE: u32 = 0o100_644;
/// 结束标记文件名。
const TRAILER: &str = "TRAILER!!!";
/// 流式拷贝缓冲区大小。
const COPY_BUF: usize = 1024 * 1024;

/// 把长度向上补齐到 4 字节所需的填充字节数。
#[must_use]
pub const fn pad4(len: u64) -> u64 {
    match len.checked_rem(4) {
        Some(0) | None => 0,
        Some(rem) => 4_u64.saturating_sub(rem),
    }
}

/// `n` 个零字节（`n` ≤ 3，用于 4 字节对齐填充）。
///
/// # Errors
///
/// `n` 超出 `usize` 时返回错误。
pub(crate) fn zeros(count: u64) -> Result<Vec<u8>, ImageError> {
    Ok(vec![0_u8; usize::try_from(count)?])
}

/// 生成 newc 头（含文件名、NUL 与填充）。文件名不得以 `/` 开头、不得含 `..` 或 NUL。
///
/// # Errors
///
/// 文件名非法时返回 [`ImageError::BadPath`]；文件超过 newc 的 32 位大小上限时返回 [`ImageError::TooLarge`]。
pub fn header(name: &str, mode: u32, size: u64, ino: u32) -> Result<Vec<u8>, ImageError> {
    if name != TRAILER
        && (name.is_empty()
            || name.starts_with('/')
            || name.contains('\0')
            || name.split('/').any(|part| part == ".." || part.is_empty()))
    {
        return Err(ImageError::BadPath(name.to_owned()));
    }
    let filesize = u32::try_from(size)?;
    let namesize = u32::try_from(name.len().checked_add(1).ok_or(ImageError::TooLarge)?)?;
    let nlink = u32::from(name != TRAILER);
    let fields = [ino, mode, 0, 0, nlink, 0, filesize, 0, 0, 0, 0, namesize, 0];
    let mut bytes = MAGIC.as_bytes().to_vec();
    for field in fields {
        bytes.extend_from_slice(format!("{field:08X}").as_bytes());
    }
    bytes.extend_from_slice(name.as_bytes());
    bytes.push(0);
    let len = u64::try_from(bytes.len())?;
    bytes.resize(
        bytes
            .len()
            .checked_add(usize::try_from(pad4(len))?)
            .ok_or(ImageError::TooLarge)?,
        0,
    );
    Ok(bytes)
}

/// 结束标记段（含填充）。
///
/// # Errors
///
/// 不会失败；保留 `Result` 以与 [`header`] 一致。
pub fn trailer() -> Result<Vec<u8>, ImageError> {
    header(TRAILER, 0, 0, 0)
}

/// 写入一个普通文件段：头 + 从 `reader` 精确拷贝 `size` 字节 + 填充。返回写出的总字节数。
///
/// # Errors
///
/// 文件名非法、大小超限、读到的字节数不足或写入失败时返回错误。
pub fn append_file<R: Read, W: Write>(
    out: &mut W,
    name: &str,
    size: u64,
    reader: &mut R,
    ino: u32,
) -> Result<u64, ImageError> {
    let head = header(name, MODE_FILE, size, ino)?;
    out.write_all(&head)?;
    let copied = copy_exact(reader, out, size)?;
    let pad = pad4(size);
    out.write_all(&zeros(pad)?)?;
    Ok(u64::try_from(head.len())?
        .saturating_add(copied)
        .saturating_add(pad))
}

/// 从 `reader` 精确拷贝 `size` 字节到 `out`。
///
/// # Errors
///
/// 读到的字节数不足或读写失败时返回错误。
pub fn copy_exact<R: Read, W: Write>(
    reader: &mut R,
    out: &mut W,
    size: u64,
) -> Result<u64, ImageError> {
    let mut buf = vec![0_u8; COPY_BUF];
    let mut left = size;
    while left > 0 {
        let want = usize::try_from(left.min(u64::try_from(COPY_BUF)?))?;
        let chunk = buf.get_mut(..want).ok_or(ImageError::TooLarge)?;
        reader.read_exact(chunk)?;
        out.write_all(chunk)?;
        left = left.saturating_sub(u64::try_from(want)?);
    }
    Ok(size)
}

#[cfg(test)]
mod tests;
