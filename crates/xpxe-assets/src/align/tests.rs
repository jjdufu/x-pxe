//! 对齐判定单元测试。

use std::io::Write as _;

use xpxe_test_support::{TestResult, ensure, ensure_eq};

use super::{Alignment, Compression, check_alignment, decompress_packages, kernel_abis};

/// 示例 `Packages` 片段。
const PACKAGES: &str = "Package: kernel-image-6.12.107+deb13-amd64-di\nVersion: 1\n\nPackage: nic-modules-6.12.107+deb13-amd64-di\n\nPackage: kernel-image-6.12.94+deb13-amd64-di\n\nPackage: kernel-image-6.12.107+deb13-amd64-di\n";

/// ABI 列表去重并判定对齐。
///
/// # Errors
///
/// 结果不符时返回错误。
#[test]
fn abis_and_alignment() -> TestResult {
    ensure_eq(
        &kernel_abis(PACKAGES),
        &vec![
            "6.12.107+deb13-amd64".to_owned(),
            "6.12.94+deb13-amd64".to_owned(),
        ],
        "abis",
    )?;
    ensure(
        check_alignment("6.12.94+deb13-amd64", PACKAGES).is_aligned(),
        "aligned",
    )?;
    let missing = check_alignment("6.12.38+deb13-amd64", PACKAGES);
    ensure(
        matches!(missing, Alignment::Misaligned { mirror_abis, .. } if mirror_abis.len() == 2),
        "misaligned lists abis",
    )?;
    Ok(())
}

/// gzip 往返与损坏输入。
///
/// # Errors
///
/// 结果不符时返回错误。
#[test]
fn gzip_roundtrip() -> TestResult {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    encoder.write_all(PACKAGES.as_bytes())?;
    let packed = encoder.finish()?;
    ensure_eq(
        decompress_packages(&packed, Compression::Gzip)?.as_str(),
        PACKAGES,
        "gzip",
    )?;
    ensure(
        decompress_packages(b"junk", Compression::Xz).is_err(),
        "bad xz",
    )?;
    ensure(
        decompress_packages(b"junk", Compression::Gzip).is_err(),
        "bad gz",
    )?;
    Ok(())
}
