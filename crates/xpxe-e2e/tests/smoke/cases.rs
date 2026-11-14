//! 端到端冒烟用例：确认目录形式的集成测试能被门禁接受并执行。

use xpxe_test_support::{TempDataDir, TestResult, ensure};

/// 临时数据目录可在端到端测试中创建并使用。
///
/// # Errors
///
/// 目录创建失败或不存在时返回错误。
#[test]
fn temp_data_dir_available() -> TestResult {
    let dir = TempDataDir::new()?;
    ensure(dir.path().is_dir(), "临时数据目录必须存在")?;
    Ok(())
}
