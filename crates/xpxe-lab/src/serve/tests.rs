//! 路径映射测试。

use std::path::{Path, PathBuf};

use xpxe_test_support::{TestResult, ensure_eq};

use super::resolve;

/// 正常路径落在根目录下，`..` 被拒绝。
///
/// # Errors
///
/// 结果不符时返回错误。
#[test]
fn resolves_inside_root() -> TestResult {
    let root = Path::new("/srv/www");
    ensure_eq(
        &resolve(root, "/d-i/linux"),
        &Some(PathBuf::from("/srv/www/d-i/linux")),
        "正常",
    )?;
    ensure_eq(
        &resolve(root, "/./boot.ipxe"),
        &Some(PathBuf::from("/srv/www/boot.ipxe")),
        "当前目录",
    )?;
    ensure_eq(&resolve(root, "/../etc/passwd"), &None, "上级目录")?;
    Ok(())
}
