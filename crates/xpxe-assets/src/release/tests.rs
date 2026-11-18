//! `Release` 与 `SHA256SUMS` 解析单元测试。

use std::time::Duration;

use xpxe_domain::Timestamp;
use xpxe_test_support::{TestResult, ensure, ensure_eq};

use super::{Release, Sha256Sums};

/// 字段、`SHA256:` 段与过旧判定。
///
/// # Errors
///
/// 结果不符时返回错误。
#[test]
fn parses_fields() -> TestResult {
    let text = "Origin: Debian\nCodename: trixie\nDate: Sat, 12 Sep 2026 07:55:41 UTC\nMD5Sum:\n d41d8cd98f00b204e9800998ecf8427e 0 main/x\nSHA256:\n 31A1A4FC99C5B0A729D261F137CD7E1599BFA6F214F83FE4E596543D8CCF2DEC 80699 main/installer-amd64/current/images/SHA256SUMS\n";
    let release = Release::parse(text)?;
    ensure_eq(&release.codename.as_deref(), &Some("trixie"), "codename")?;
    let date = release.date.ok_or("no date")?;
    ensure_eq(&date.unix_seconds(), &1_789_199_741, "date")?;
    let entry = release
        .sha256_of("main/installer-amd64/current/images/SHA256SUMS")
        .ok_or("missing entry")?;
    ensure_eq(&entry.size, &80_699, "size")?;
    ensure(entry.sha256.starts_with("31a1a4fc"), "lowercased")?;
    ensure(release.sha256_of("main/x").is_none(), "md5 section ignored")?;
    ensure(
        !release.is_stale(date.saturating_add(Duration::from_hours(24))),
        "fresh",
    )?;
    ensure(
        release.is_stale(date.saturating_add(Duration::from_hours(24 * 31))),
        "stale",
    )?;
    ensure(Release::parse("Date: yesterday\n").is_err(), "bad date")?;
    ensure(Release::parse("SHA256:\n zz 1 a\n").is_err(), "bad hash")?;
    Ok(())
}

/// `SHA256SUMS` 行与 `./` 前缀。
///
/// # Errors
///
/// 结果不符时返回错误。
#[test]
fn parses_sums() -> TestResult {
    let sha = "a".repeat(64);
    let sums = Sha256Sums::parse(&format!(
        "{sha}  ./netboot/linux\n{sha} *firmware.cpio.gz\n"
    ))?;
    ensure_eq(&sums.get("netboot/linux"), &Some(sha.as_str()), "plain")?;
    ensure_eq(
        &sums.get("./netboot/linux"),
        &Some(sha.as_str()),
        "dot slash",
    )?;
    ensure_eq(
        &sums.get("firmware.cpio.gz"),
        &Some(sha.as_str()),
        "binary marker",
    )?;
    ensure(Sha256Sums::parse("nothex  x\n").is_err(), "bad line")?;
    let _unused = Timestamp::from_unix_seconds(0);
    Ok(())
}
