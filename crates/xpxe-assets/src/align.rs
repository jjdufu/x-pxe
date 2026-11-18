//! 内核 ABI 与镜像源对齐：`main/debian-installer/binary-amd64/Packages` 中的 `kernel-image-<ABI>-di`。

use std::io::{Read as _, Write};

use serde::Serialize;

use crate::error::{AssetsError, AssetsResult, fail};

/// 解压后 `Packages` 的大小上限（防解压炸弹）。
pub const PACKAGES_MAX: usize = 64 * 1024 * 1024;

/// `Packages` 压缩格式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compression {
    /// `.xz`。
    Xz,
    /// `.gz`。
    Gzip,
}

/// 对齐结论。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Alignment {
    /// 镜像中存在与 netboot 内核一致的 `kernel-image-<ABI>-di`。
    Aligned {
        /// netboot 内核 ABI。
        abi: String,
    },
    /// 不一致：列出镜像中现有的 ABI。
    Misaligned {
        /// netboot 内核 ABI。
        abi: String,
        /// 镜像中现有的 ABI。
        mirror_abis: Vec<String>,
    },
}

impl Alignment {
    /// 是否已对齐。
    #[must_use]
    pub const fn is_aligned(&self) -> bool {
        matches!(self, Self::Aligned { .. })
    }
}

/// 解压 `Packages.xz` / `Packages.gz`，超过 [`PACKAGES_MAX`] 时中止。
///
/// # Errors
///
/// 解压失败、超限或不是 UTF-8 时返回错误。
pub fn decompress_packages(data: &[u8], compression: Compression) -> AssetsResult<String> {
    let mut out = Vec::new();
    match compression {
        Compression::Xz => {
            let mut sink = Limited {
                inner: &mut out,
                limit: PACKAGES_MAX,
            };
            let mut input = data;
            lzma_rs::xz_decompress(&mut input, &mut sink).map_err(|error| {
                AssetsError::new("bad_packages", format!("xz 解压失败：{error}"))
            })?;
        }
        Compression::Gzip => {
            let limit = u64::try_from(PACKAGES_MAX)
                .unwrap_or(u64::MAX)
                .saturating_add(1);
            let _read = flate2::read::GzDecoder::new(data)
                .take(limit)
                .read_to_end(&mut out)
                .map_err(|error| {
                    AssetsError::new("bad_packages", format!("gzip 解压失败：{error}"))
                })?;
            if out.len() > PACKAGES_MAX {
                return Err(fail("bad_packages", "Packages 解压后超过 64 MiB"));
            }
        }
    }
    String::from_utf8(out).map_err(|_error| fail("bad_packages", "Packages 不是 UTF-8"))
}

/// 列出 `Packages` 中全部 `kernel-image-<ABI>-di` 的 ABI（去重、按出现顺序）。
#[must_use]
pub fn kernel_abis(packages: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in packages.lines() {
        let abi = line
            .strip_prefix("Package: kernel-image-")
            .and_then(|rest| rest.trim_end().strip_suffix("-di"));
        if let Some(found) = abi
            && !out.iter().any(|known| known == found)
        {
            out.push(found.to_owned());
        }
    }
    out
}

/// 判定 netboot ABI 是否与镜像对齐。
#[must_use]
pub fn check_alignment(abi: &str, packages: &str) -> Alignment {
    let mirror_abis = kernel_abis(packages);
    if mirror_abis.iter().any(|known| known == abi) {
        Alignment::Aligned {
            abi: abi.to_owned(),
        }
    } else {
        Alignment::Misaligned {
            abi: abi.to_owned(),
            mirror_abis,
        }
    }
}

/// 有上限的写入器。
struct Limited<'out> {
    /// 输出。
    inner: &'out mut Vec<u8>,
    /// 上限。
    limit: usize,
}

impl Write for Limited<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if self.inner.len().saturating_add(buf.len()) > self.limit {
            return Err(std::io::Error::other("Packages 解压后超过上限"));
        }
        self.inner.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests;
