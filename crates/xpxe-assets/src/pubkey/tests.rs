//! `OpenPGP` 公钥解析测试。

use xpxe_test_support::{TestResult, ensure, ensure_eq};

use super::parse_public_key;

/// 测试用 Ed25519 公钥装甲。
const TEST_KEY: &str = include_str!("../../tests/fixtures/test-key.asc");

/// 已知测试钥的 v4 指纹。
const TEST_FINGERPRINT: &str = "6078FE94008A850B0663E3404E498AFA243BDC40";

/// 解析测试钥得到标准指纹。
///
/// # Errors
///
/// 解析失败或指纹不符时返回错误。
#[test]
fn parses_test_key_fingerprint() -> TestResult {
    let info = parse_public_key(TEST_KEY)?;
    ensure_eq(info.fingerprint(), TEST_FINGERPRINT, "fingerprint")?;
    Ok(())
}

/// 拒绝缺少头尾或注入内容的输入。
///
/// # Errors
///
/// 非法输入被接受时返回错误。
#[test]
fn rejects_malformed_armor() -> TestResult {
    ensure(parse_public_key("").is_err(), "empty")?;
    ensure(
        parse_public_key(
            "-----BEGIN PGP PUBLIC KEY BLOCK-----\nxxx\n-----END PGP PUBLIC KEY BLOCK-----",
        )
        .is_err(),
        "bad b64",
    )?;
    ensure(parse_public_key("not a key $(id)").is_err(), "junk")?;
    Ok(())
}
