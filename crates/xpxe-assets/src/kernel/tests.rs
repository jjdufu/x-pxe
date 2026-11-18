//! bzImage 版本串单元测试。

use xpxe_test_support::{TestResult, ensure_eq};

use super::kernel_version;

/// 构造最小 bzImage 头：`HdrS` + 指针 + 版本串。
fn header(pointer: u16, version: &[u8]) -> Vec<u8> {
    let mut data = vec![0_u8; 0x600];
    if let Some(slot) = data.get_mut(0x202..0x206) {
        slot.copy_from_slice(b"HdrS");
    }
    if let Some(slot) = data.get_mut(0x20E..0x210) {
        slot.copy_from_slice(&pointer.to_le_bytes());
    }
    let start = usize::from(pointer).saturating_add(0x200);
    if let Some(slot) = data.get_mut(start..start.saturating_add(version.len())) {
        slot.copy_from_slice(version);
    }
    data
}

/// 正常、非 bzImage、越界、零指针、无终止符。
///
/// # Errors
///
/// 结果不符时返回错误。
#[test]
fn header_cases() -> TestResult {
    let good = kernel_version(&header(0x100, b"6.12.1+deb13-amd64 (x) #1\0"))?;
    ensure_eq(good.abi.as_str(), "6.12.1+deb13-amd64", "abi")?;
    let code = |data: &[u8]| kernel_version(data).err().map(|error| error.code());
    ensure_eq(&code(&[0_u8; 0x400]), &Some("not_bzimage"), "no magic")?;
    ensure_eq(
        &code(&header(0x3000, b"")),
        &Some("bad_kernel"),
        "out of range",
    )?;
    ensure_eq(&code(&header(0, b"")), &Some("bad_kernel"), "zero pointer")?;
    ensure_eq(
        &code(&header(0x100, b"$(id)\0")),
        &Some("bad_kernel"),
        "not abi",
    )?;
    let mut unterminated = header(0x100, b"6.1");
    unterminated.truncate(0x303);
    ensure_eq(&code(&unterminated), &Some("bad_kernel"), "unterminated")?;
    Ok(())
}
