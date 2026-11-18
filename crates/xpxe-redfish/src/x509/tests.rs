//! 证书摘要解析。

use xpxe_test_support::{TestResult, ensure, ensure_eq, fixture_bytes};

use super::summarize;

/// 测试证书的主题、颁发者（自签相同）与有效期。
///
/// # Errors
///
/// 解析结果不符时返回错误。
#[test]
fn self_signed_fixture() -> TestResult {
    let der = fixture_bytes(env!("CARGO_MANIFEST_DIR"), "tls/bmc.der")?;
    let summary = summarize(&der)?;
    ensure_eq(
        &summary.subject.as_str(),
        &"CN=127.0.0.1, O=x-pxe test bmc",
        "主题",
    )?;
    ensure_eq(&summary.issuer, &summary.subject, "自签")?;
    ensure(
        summary.not_before.starts_with("2026-") && summary.not_after.starts_with("2036-"),
        "有效期",
    )?;
    ensure(summarize(b"\x30\x05garbage").is_err(), "损坏 DER")?;
    Ok(())
}
