//! `xpxe-test-support` 自身的单元测试。

use super::{TempDataDir, TestResult, ensure, ensure_eq};

/// `ensure` 在条件成立时通过、不成立时返回带消息的错误。
///
/// # Errors
///
/// 行为不符合预期时返回错误。
#[test]
fn ensure_reports_message() -> TestResult {
    ensure(true, "真条件")?;
    let failed = ensure(false, "假条件");
    ensure(
        failed.is_err_and(|failure| failure.message() == "假条件"),
        "假条件必须返回原消息",
    )?;
    Ok(())
}

/// `ensure_eq` 失败信息里同时包含左右两侧的值。
///
/// # Errors
///
/// 行为不符合预期时返回错误。
#[test]
fn ensure_eq_shows_both_sides() -> TestResult {
    ensure_eq(&1_u8, &1_u8, "相等")?;
    let failed = ensure_eq(&1_u8, &2_u8, "不等");
    ensure(
        failed.is_err_and(|failure| {
            failure.message().contains('1') && failure.message().contains('2')
        }),
        "失败信息必须包含两侧的值",
    )?;
    Ok(())
}

/// 临时目录创建后存在，析构后被删除。
///
/// # Errors
///
/// 行为不符合预期时返回错误。
#[test]
fn temp_dir_is_removed_on_drop() -> TestResult {
    let dir = TempDataDir::new()?;
    let kept = dir.path().to_path_buf();
    ensure(kept.is_dir(), "临时目录必须存在")?;
    drop(dir);
    ensure(!kept.exists(), "析构后临时目录必须删除")?;
    Ok(())
}
