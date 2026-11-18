//! RFC 1321 测试向量。

use xpxe_test_support::{TestResult, ensure_eq};

use super::hex_digest;

/// 空串与 `abc` 的标准摘要。
///
/// # Errors
///
/// 摘要不符时返回错误。
#[test]
fn rfc1321_vectors() -> TestResult {
    ensure_eq(
        hex_digest(b"").as_str(),
        "d41d8cd98f00b204e9800998ecf8427e",
        "empty",
    )?;
    ensure_eq(
        hex_digest(b"abc").as_str(),
        "900150983cd24fb0d6963f7d28e17f72",
        "abc",
    )?;
    ensure_eq(
        hex_digest(b"abcdefghijklmnopqrstuvwxyz").as_str(),
        "c3fcd3d76192e4007dfb496cca67e13b",
        "alpha",
    )?;
    Ok(())
}
