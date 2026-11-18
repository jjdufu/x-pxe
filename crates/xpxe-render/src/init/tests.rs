//! 初始化渲染内部单元测试。

use xpxe_test_support::{TestResult, ensure_eq};

use super::sha256_hex;

/// 摘要与 `sha256sum` 一致（agent 用同一算法校验）。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn sha256_matches_coreutils() -> TestResult {
    ensure_eq(
        &sha256_hex(b"").as_str(),
        &"e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        "空串摘要",
    )?;
    Ok(())
}
