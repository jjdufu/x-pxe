//! `sh_quote` 转义。

use xpxe_test_support::{TestResult, ensure_eq};

use super::sh_quote;

/// 单引号与普通文本。
///
/// # Errors
///
/// 转义不符时返回错误。
#[test]
fn quotes_and_escapes() -> TestResult {
    ensure_eq(sh_quote("abc").as_str(), "'abc'", "plain")?;
    ensure_eq(sh_quote("a'b").as_str(), "'a'\\''b'", "quote")?;
    ensure_eq(sh_quote("$(id)").as_str(), "'$(id)'", "sub")?;
    Ok(())
}
