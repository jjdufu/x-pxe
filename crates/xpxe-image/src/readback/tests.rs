//! 读回解析器对损坏输入的处理。

use xpxe_test_support::{TestResult, ensure};

use super::{read_fat, read_iso};

/// 空镜像与签名错误被拒绝，不会 panic。
///
/// # Errors
///
/// 未拒绝时返回错误。
#[test]
fn rejects_garbage() -> TestResult {
    ensure(read_iso(&[]).is_err(), "空 ISO")?;
    ensure(read_iso(&vec![0_u8; 40 * 2048]).is_err(), "无签名 ISO")?;
    ensure(read_fat(&[]).is_err(), "空 FAT")?;
    ensure(read_fat(&vec![0_u8; 4096]).is_err(), "无 55AA FAT")?;
    Ok(())
}
