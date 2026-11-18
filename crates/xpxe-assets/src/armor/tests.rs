//! 装甲与内联签名拆分单元测试。

use xpxe_test_support::{TestResult, ensure, ensure_eq};

use super::{crc24, decode_blocks, split_cleartext};

/// 空输入的 CRC24 等于初值。
///
/// # Errors
///
/// 结果不符时返回错误。
#[test]
fn crc24_of_empty_is_init() -> TestResult {
    ensure_eq(&crc24(&[]), &[0x00, 0xB7, 0x04, 0xCE], "crc24")?;
    Ok(())
}

/// 正确 CRC 通过，错误 CRC 被拒；块外内容忽略。
///
/// # Errors
///
/// 结果不符时返回错误。
#[test]
fn crc_checked() -> TestResult {
    let good = "junk\n-----BEGIN PGP SIGNATURE-----\n\nAQID\n=Z2GT\n-----END PGP SIGNATURE-----\n";
    let blocks = decode_blocks(good, "PGP SIGNATURE")?;
    ensure_eq(&blocks, &vec![vec![1_u8, 2, 3]], "decoded")?;
    let bad = good.replace("=Z2GT", "=AAAA");
    ensure(decode_blocks(&bad, "PGP SIGNATURE").is_err(), "bad crc")?;
    ensure(
        decode_blocks("-----BEGIN PGP SIGNATURE-----\nAQID\n", "PGP SIGNATURE").is_err(),
        "unterminated",
    )?;
    Ok(())
}

/// 规范化：去行尾空白、撤销破折号转义、`\r\n` 连接、末行无换行。
///
/// # Errors
///
/// 结果不符时返回错误。
#[test]
fn cleartext_canonicalization() -> TestResult {
    let raw = "-----BEGIN PGP SIGNED MESSAGE-----\r\nHash: SHA256\r\n\r\nA: b  \r\n- - x\r\n-----BEGIN PGP SIGNATURE-----\r\n\r\nAQID\r\n-----END PGP SIGNATURE-----\r\n";
    let parts = split_cleartext(raw)?;
    ensure_eq(parts.text.as_str(), "A: b  \n- x\n", "text")?;
    ensure_eq(
        parts.canonical.as_slice(),
        b"A: b\r\n- x".as_slice(),
        "canonical",
    )?;
    ensure_eq(parts.signature.as_slice(), [1_u8, 2, 3].as_slice(), "sig")?;
    ensure(split_cleartext("Origin: x\n").is_err(), "not signed")?;
    Ok(())
}
