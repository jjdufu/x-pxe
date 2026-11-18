//! iPXE 格式与内核 ABI（真实二进制夹具）。

use xpxe_assets::{AssetKind, kernel_version, verify_ipxe};
use xpxe_test_support::{TestResult, ensure, ensure_eq};

use crate::support::{code, fixture};

/// 真实 snponly.efi、undionly.kpxe 通过；HTML 404、ARM64 EFI、截断文件被拒。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn ipxe_formats() -> TestResult {
    let efi = verify_ipxe(AssetKind::IpxeSnponlyX64, &fixture("ipxe/snponly.efi")?)?;
    ensure_eq(efi.format, "PE32+ x86-64 EFI application", "efi")?;
    ensure(
        verify_ipxe(AssetKind::IpxeEfiX64, &fixture("ipxe/snponly.efi")?).is_ok(),
        "efi kind",
    )?;
    ensure(
        verify_ipxe(AssetKind::IpxeUndionly, &fixture("ipxe/undionly.kpxe")?).is_ok(),
        "kpxe",
    )?;
    let html = fixture("ipxe/not-found.html")?;
    for kind in [AssetKind::IpxeEfiX64, AssetKind::IpxeUndionly] {
        ensure_eq(
            &code(&verify_ipxe(kind, &html)),
            &Some("bad_format"),
            "html",
        )?;
    }
    let arm = verify_ipxe(
        AssetKind::IpxeEfiX64,
        &fixture("ipxe/arm64-snponly-head.efi")?,
    );
    ensure(
        arm.as_ref()
            .err()
            .is_some_and(|error| error.message().contains("0xaa64")),
        "arm64",
    )?;
    ensure_eq(
        &code(&verify_ipxe(
            AssetKind::IpxeEfiX64,
            &fixture("ipxe/truncated.efi")?,
        )),
        &Some("bad_format"),
        "truncated",
    )?;
    ensure_eq(
        &code(&verify_ipxe(
            AssetKind::IpxeUndionly,
            &fixture("ipxe/snponly.efi")?,
        )),
        &Some("bad_format"),
        "efi is not kpxe",
    )?;
    ensure_eq(
        &code(&verify_ipxe(AssetKind::DebianFirmwareCpio, &html)),
        &Some("bad_kind"),
        "not ipxe",
    )?;
    Ok(())
}

/// 真实 bzImage 头部与人工旧版本头部；非 bzImage 被拒。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn kernel_abi() -> TestResult {
    let real = kernel_version(&fixture("debian/linux-head.bin")?)?;
    ensure_eq(real.abi.as_str(), "6.12.107+deb13-amd64", "real")?;
    ensure(real.full.contains("debian-kernel"), "full string")?;
    let old = kernel_version(&fixture("debian/linux-head-old.bin")?)?;
    ensure_eq(old.abi.as_str(), "6.12.38+deb13-amd64", "old")?;
    ensure_eq(
        &code(&kernel_version(&fixture("ipxe/snponly.efi")?)),
        &Some("not_bzimage"),
        "efi",
    )?;
    Ok(())
}
