//! 信任根与签名验证单元测试（夹具见 `tests/fixtures/`）。

use xpxe_domain::Timestamp;
use xpxe_test_support::{TestResult, ensure, ensure_eq, fixture_bytes};

use super::{TrustStore, parse_pins};

/// 测试公钥环。
const TEST_KEYS: &str = include_str!("../../tests/fixtures/pgp/test-keyring.asc");
/// 测试白名单。
const TEST_PINS: &str = include_str!("../../tests/fixtures/pgp/test-fingerprints.txt");
/// RSA 测试签名（SHA-256）。
const RSA_SIGNED: &str = include_str!("../../tests/fixtures/pgp/InRelease-rsa");
/// Ed25519 测试签名（SHA-512）。
const ED_SIGNED: &str = include_str!("../../tests/fixtures/pgp/InRelease-ed25519");

/// 2026-11-18T00:00:00Z。
const NOW: Timestamp = Timestamp::from_unix_seconds(1_795_478_400);

/// 内置信任根可加载，并包含全部白名单指纹。
///
/// # Errors
///
/// 加载失败时返回错误。
#[test]
fn embedded_trust_loads() -> TestResult {
    let trust = TrustStore::debian()?;
    ensure_eq(&trust.fingerprints().count(), &11, "pin count")?;
    ensure(
        trust
            .fingerprints()
            .any(|fpr| fpr == "B8E5F13176D2A7A75220028078DBA3BC47EF2265"),
        "trixie subkey pinned",
    )?;
    Ok(())
}

/// 真实 `InRelease` 由 trixie 归档子钥签名并验证通过。
///
/// # Errors
///
/// 验证失败时返回错误。
#[test]
fn real_inrelease_verifies() -> TestResult {
    // 真实 Debian trixie `InRelease`（2026-09-12）。
    let raw = fixture_bytes(env!("CARGO_MANIFEST_DIR"), "debian/InRelease")?;
    let signed = TrustStore::debian()?.verify_cleartext(std::str::from_utf8(&raw)?, NOW)?;
    ensure(signed.text.starts_with("Origin: Debian\n"), "text start")?;
    // 归档同时由 bookworm 与 trixie 自动签名钥签名，取第一个可信且通过的签名。
    ensure(
        [
            "B8B80B5B623EAB6AD8775C45B7C5D7D6350947F8",
            "04B54C3CDCA79751B16BC6B5225629DF75B188BD",
        ]
        .contains(&signed.signer.primary_fingerprint.as_str()),
        "primary is a Debian archive key",
    )?;
    ensure(
        signed.signer.fingerprint != signed.signer.primary_fingerprint,
        "signed by subkey",
    )?;
    Ok(())
}

/// RSA/SHA-256 与 Ed25519/SHA-512 测试签名均通过；行尾空白与破折号转义按规范处理。
///
/// # Errors
///
/// 验证失败时返回错误。
#[test]
fn generated_keys_verify() -> TestResult {
    let trust = TrustStore::from_parts(&[TEST_KEYS], TEST_PINS)?;
    let rsa = trust.verify_cleartext(RSA_SIGNED, NOW)?;
    ensure(rsa.text.contains("\n- dashed line\n"), "dash unescaped")?;
    ensure(rsa.signer.label.contains("RSA"), "rsa label")?;
    let ed = trust.verify_cleartext(ED_SIGNED, NOW)?;
    ensure(ed.signer.label.contains("Ed25519"), "ed label")?;
    Ok(())
}

/// 白名单格式错误被拒。
///
/// # Errors
///
/// 非法白名单被接受时返回错误。
#[test]
fn bad_pins_rejected() -> TestResult {
    ensure(parse_pins("ABC - short").is_err(), "short fpr")?;
    ensure(
        parse_pins("04B54C3CDCA79751B16BC6B5225629DF75B188BD tomorrow x").is_err(),
        "bad date",
    )?;
    ensure_eq(&parse_pins("# c\n\n")?.len(), &0, "comments")?;
    Ok(())
}
