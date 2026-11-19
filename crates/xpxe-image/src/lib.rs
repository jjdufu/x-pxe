//! x-pxe 的纯 Rust 镜像写出器。
//!
//! - [`fat`]：在内存中生成 FAT12/16 文件系统镜像（支持长文件名），用作 El Torito 的 ESP 映像。
//! - [`iso`]：生成 ISO9660 + El Torito 引导光盘，只含一个 UEFI（平台 0xEF）no-emulation 引导项。
//! - [`session`]：Redfish 会话引导 ISO（`ipxe.efi` + `autoexec.ipxe`）。
//! - [`readback`]：独立于写出器的 ISO/FAT 读回解析器，用于核对生成结果。
//! - [`iso_read`]：流式读取任意 ISO（Rock Ridge / Joliet / ISO9660），识别发行版并抽取文件（Step 14）。
//! - [`cpio`]：newc cpio 写入器，用于把整份 ISO 追加进 initrd。
//!
//! 输出是确定性的：所有时间戳由调用方给出的 [`Timestamp`] 决定，相同输入得到相同字节。
#![doc(test(attr(deny(warnings), forbid(unsafe_code))))]

pub mod cpio;
pub mod fat;
pub mod iso;
pub mod iso_read;
pub mod readback;
pub mod session;

/// 镜像内统一使用的时间戳（UTC）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timestamp {
    /// 年（1980–2107）。
    pub year: u16,
    /// 月（1–12）。
    pub month: u8,
    /// 日（1–31）。
    pub day: u8,
    /// 时（0–23）。
    pub hour: u8,
    /// 分（0–59）。
    pub minute: u8,
    /// 秒（0–59）。
    pub second: u8,
}

impl Timestamp {
    /// 校验各字段的取值范围。
    ///
    /// # Errors
    ///
    /// 任一字段越界时返回 [`ImageError::BadTimestamp`]。
    pub const fn validate(self) -> Result<Self, ImageError> {
        let valid = self.year >= 1980
            && self.year <= 2107
            && self.month >= 1
            && self.month <= 12
            && self.day >= 1
            && self.day <= 31
            && self.hour <= 23
            && self.minute <= 59
            && self.second <= 59;
        if valid {
            Ok(self)
        } else {
            Err(ImageError::BadTimestamp)
        }
    }
}

/// 镜像生成失败的原因。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageError {
    /// 内容超出该格式能表示的大小。
    TooLarge,
    /// 时间戳越界。
    BadTimestamp,
    /// 路径为空、含非法字符或与已有条目冲突。
    BadPath(String),
    /// 读回解析时镜像结构不合法。
    Malformed(String),
    /// 读写失败（流式读取 ISO 或写出抽取产物时）。
    Io(String),
}

impl std::fmt::Display for ImageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooLarge => f.write_str("内容超出镜像格式能表示的大小"),
            Self::BadTimestamp => f.write_str("时间戳越界"),
            Self::BadPath(path) => write!(f, "非法路径：{path}"),
            Self::Malformed(what) => write!(f, "镜像结构不合法：{what}"),
            Self::Io(what) => write!(f, "读写失败：{what}"),
        }
    }
}

impl std::error::Error for ImageError {}

impl From<std::io::Error> for ImageError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value.to_string())
    }
}

impl From<std::num::TryFromIntError> for ImageError {
    fn from(_value: std::num::TryFromIntError) -> Self {
        Self::TooLarge
    }
}

/// 带溢出检查的 `usize` 运算，溢出即 [`ImageError::TooLarge`]。
pub(crate) mod math {
    use super::ImageError;

    /// `a + b`。
    ///
    /// # Errors
    ///
    /// 溢出时返回 [`ImageError::TooLarge`]。
    pub(crate) const fn add(lhs: usize, rhs: usize) -> Result<usize, ImageError> {
        match lhs.checked_add(rhs) {
            Some(sum) => Ok(sum),
            None => Err(ImageError::TooLarge),
        }
    }

    /// `a * b`。
    ///
    /// # Errors
    ///
    /// 溢出时返回 [`ImageError::TooLarge`]。
    pub(crate) const fn mul(lhs: usize, rhs: usize) -> Result<usize, ImageError> {
        match lhs.checked_mul(rhs) {
            Some(product) => Ok(product),
            None => Err(ImageError::TooLarge),
        }
    }

    /// 向上取整除法；除数为 0 时视为溢出。
    ///
    /// # Errors
    ///
    /// 除数为 0 时返回 [`ImageError::TooLarge`]。
    pub(crate) const fn div_ceil(lhs: usize, rhs: usize) -> Result<usize, ImageError> {
        match lhs.checked_div(rhs) {
            Some(quotient) => match lhs.checked_rem(rhs) {
                Some(0) | None => Ok(quotient),
                Some(_) => add(quotient, 1),
            },
            None => Err(ImageError::TooLarge),
        }
    }

    /// 转换为 `u16`。
    ///
    /// # Errors
    ///
    /// 超出范围时返回 [`ImageError::TooLarge`]。
    pub(crate) fn to_u16(value: usize) -> Result<u16, ImageError> {
        Ok(u16::try_from(value)?)
    }

    /// 转换为 `u32`。
    ///
    /// # Errors
    ///
    /// 超出范围时返回 [`ImageError::TooLarge`]。
    pub(crate) fn to_u32(value: usize) -> Result<u32, ImageError> {
        Ok(u32::try_from(value)?)
    }

    /// 在 `buf` 的 `offset` 处写入 `bytes`。
    ///
    /// # Errors
    ///
    /// 越界时返回 [`ImageError::TooLarge`]。
    pub(crate) fn put(buf: &mut [u8], offset: usize, bytes: &[u8]) -> Result<(), ImageError> {
        let end = add(offset, bytes.len())?;
        buf.get_mut(offset..end)
            .ok_or(ImageError::TooLarge)?
            .copy_from_slice(bytes);
        Ok(())
    }
}

#[cfg(test)]
mod tests;
