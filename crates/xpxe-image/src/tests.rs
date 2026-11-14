//! 时间戳校验测试。

use xpxe_test_support::{TestResult, ensure};

use super::Timestamp;

/// 合法时间通过，越界月份与 1980 年之前被拒绝。
///
/// # Errors
///
/// 校验结果不符时返回错误。
#[test]
fn timestamp_bounds() -> TestResult {
    let ok = Timestamp {
        year: 2026,
        month: 11,
        day: 14,
        hour: 9,
        minute: 30,
        second: 0,
    };
    ensure(ok.validate().is_ok(), "合法时间")?;
    ensure(
        Timestamp { month: 13, ..ok }.validate().is_err(),
        "月份越界",
    )?;
    ensure(
        Timestamp { year: 1979, ..ok }.validate().is_err(),
        "1980 之前",
    )?;
    Ok(())
}
