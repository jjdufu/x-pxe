//! 列值转换单元测试。

use xpxe_domain::Timestamp;
use xpxe_test_support::{TestResult, ensure, ensure_eq};

use super::{ct_eq, parse_ts, random_token, ts};

/// 时间写成定长 RFC 3339 并能读回。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn rfc3339_round_trip() -> TestResult {
    let at = Timestamp::from_unix_seconds(1_795_000_000);
    let text = ts(at)?;
    ensure_eq(&text.len(), &20, "定长")?;
    ensure(text.ends_with('Z'), "UTC")?;
    ensure_eq(&parse_ts(&text)?, &at, "往返")?;
    ensure(
        ts(Timestamp::from_unix_seconds(9))? < ts(Timestamp::from_unix_seconds(10))?,
        "文本序与时间序一致",
    )?;
    Ok(())
}

/// 随机令牌长度与比较。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn token_and_compare() -> TestResult {
    let first = random_token()?;
    let second = random_token()?;
    ensure_eq(&first.len(), &43, "32 字节 base64url 无填充")?;
    ensure(first != second, "两次不同")?;
    ensure(ct_eq(b"abc", b"abc"), "相等")?;
    ensure(!ct_eq(b"abc", b"abd"), "不等")?;
    ensure(!ct_eq(b"abc", b"ab"), "长度不同")?;
    Ok(())
}
