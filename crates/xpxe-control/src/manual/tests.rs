//! 手动引导辅助函数：边写边算摘要与 ISO 识别。

use std::io::{Cursor, Write as _};

use sha2::{Digest as _, Sha256};
use xpxe_test_support::{TestResult, ensure, ensure_eq};

use super::{HashWriter, inspect};

/// 写入器摘要与直接计算一致。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn hash_writer_matches_digest() -> TestResult {
    let mut writer = HashWriter::new(Vec::new());
    writer.write_all(b"hello ")?;
    writer.write_all(b"iso")?;
    let (sha, size, inner) = writer.finish();
    ensure_eq(&sha, &hex::encode(Sha256::digest(b"hello iso")), "摘要")?;
    ensure_eq(&size, &9, "字节数")?;
    ensure_eq(&inner, &b"hello iso".to_vec(), "原样写出")?;
    Ok(())
}

/// 非 ISO 内容只标记为未识别，不报错。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn garbage_is_unknown() -> TestResult {
    let report = inspect(Cursor::new(vec![0_u8; 64 * 1024]));
    ensure_eq(
        &report.get("family").and_then(|value| value.as_str()),
        &Some("unknown"),
        "未识别",
    )?;
    ensure(report.get("error").is_some(), "带错误说明")?;
    Ok(())
}
