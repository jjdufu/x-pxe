//! iPXE 格式检查单元测试（真实二进制见集成测试）。

use xpxe_test_support::{TestResult, ensure, ensure_eq};

use super::{check_undionly, find_version};

/// 版本串提取。
///
/// # Errors
///
/// 结果不符时返回错误。
#[test]
fn version_string() -> TestResult {
    ensure_eq(
        &find_version(b"\0iPXE> \0iPXE 2.0.0+ (ge6555)\0").as_deref(),
        &Some("iPXE 2.0.0+ (ge6555)"),
        "found",
    )?;
    ensure(
        find_version(b"iPXE> prompt").is_none(),
        "prompt is not version",
    )?;
    Ok(())
}

/// `undionly.kpxe` 大小与前缀。
///
/// # Errors
///
/// 结果不符时返回错误。
#[test]
fn undionly_rules() -> TestResult {
    let mut data = vec![0_u8; 64 * 1024];
    ensure(check_undionly(&data).is_err(), "no prefix")?;
    if let Some(head) = data.get_mut(..5) {
        head.copy_from_slice(&[0xEA, 0x05, 0x00, 0xC0, 0x07]);
    }
    ensure(check_undionly(&data).is_ok(), "prefix ok")?;
    data.truncate(1024);
    ensure(check_undionly(&data).is_err(), "too small")?;
    Ok(())
}
