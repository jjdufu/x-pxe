//! 账户单元测试：退避序列与 argon2 参数。

use xpxe_test_support::{TestResult, ensure, ensure_eq};

use super::{BACKOFF_CAP_SECS, backoff_delay, hash_password};

/// 前 4 次不退避，第 5 次起 2、4、8 … 秒，上限 15 min。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn backoff_sequence() -> TestResult {
    let seq: Vec<u64> = (0..=16).map(backoff_delay).collect();
    ensure_eq(
        &seq,
        &vec![
            0, 0, 0, 0, 0, 2, 4, 8, 16, 32, 64, 128, 256, 512, 900, 900, 900,
        ],
        "退避序列",
    )?;
    ensure_eq(&backoff_delay(u64::MAX), &BACKOFF_CAP_SECS, "极大值封顶")?;
    Ok(())
}

/// PHC 字符串带 argon2id、m=65536、t=3、p=1；口令长度 12–256。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn argon2_parameters_and_length() -> TestResult {
    let phc = hash_password("correct horse battery")?;
    ensure(phc.starts_with("$argon2id$v=19$m=65536,t=3,p=1$"), &phc)?;
    ensure(hash_password("short").is_err(), "过短")?;
    ensure(hash_password(&"x".repeat(257)).is_err(), "过长")?;
    ensure(hash_password(&"口".repeat(12)).is_ok(), "按字符计数")?;
    Ok(())
}
