//! 对齐判定（夹具 Packages.xz / Packages.gz）。

use xpxe_assets::{Alignment, Compression, check_alignment, decompress_packages, kernel_version};
use xpxe_test_support::{TestResult, ensure, ensure_eq};

use crate::support::fixture;

/// netboot 与镜像一致时已对齐；旧 netboot 未对齐并列出镜像现有 ABI。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn packages_alignment() -> TestResult {
    for (path, compression) in [
        ("debian/Packages.xz", Compression::Xz),
        ("debian/Packages.gz", Compression::Gzip),
    ] {
        let text = decompress_packages(&fixture(path)?, compression)?;
        let current = kernel_version(&fixture("debian/linux-head.bin")?)?;
        ensure(check_alignment(&current.abi, &text).is_aligned(), "aligned")?;
        let old = kernel_version(&fixture("debian/linux-head-old.bin")?)?;
        let Alignment::Misaligned { mirror_abis, .. } = check_alignment(&old.abi, &text) else {
            return Err("old netboot must be misaligned".into());
        };
        ensure(
            mirror_abis.contains(&"6.12.107+deb13-amd64".to_owned()),
            "lists mirror abi",
        )?;
        ensure_eq(&mirror_abis.len(), &2, "two abis in fixture")?;
    }
    Ok(())
}
