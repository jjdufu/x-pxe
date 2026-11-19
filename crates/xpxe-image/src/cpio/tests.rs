//! newc 写入器测试。

use xpxe_test_support::{TestResult, ensure, ensure_eq};

use super::{append_file, header, pad4, trailer};

/// 头部为 110 字节固定字段 + 文件名 + NUL，整体 4 字节对齐；字段为大写十六进制。
///
/// # Errors
///
/// 结果不符时返回错误。
#[test]
fn header_layout() -> TestResult {
    let head = header("proxmox.iso", 0o100_644, 5, 1)?;
    ensure(head.len().is_multiple_of(4), "4 字节对齐")?;
    ensure(head.starts_with(b"070701"), "魔数")?;
    ensure_eq(&head.get(54..62), &Some(&b"00000005"[..]), "文件大小字段")?;
    ensure_eq(
        &head.get(94..102),
        &Some(&b"0000000C"[..]),
        "文件名长度含 NUL",
    )?;
    ensure(
        head.get(110..)
            .is_some_and(|rest| rest.starts_with(b"proxmox.iso\0")),
        "文件名",
    )?;
    ensure(header("/abs", 0, 0, 0).is_err(), "拒绝绝对路径")?;
    ensure(header("a/../b", 0, 0, 0).is_err(), "拒绝 ..")?;
    ensure(
        header("big", 0, u64::from(u32::MAX).saturating_add(1), 0).is_err(),
        "拒绝超过 4 GiB",
    )?;
    Ok(())
}

/// 文件段 = 头 + 内容 + 填充；结束标记名为 TRAILER!!!。
///
/// # Errors
///
/// 结果不符时返回错误。
#[test]
fn file_and_trailer() -> TestResult {
    let mut out = Vec::new();
    let mut data: &[u8] = b"hello";
    let written = append_file(&mut out, "a.txt", 5, &mut data, 7)?;
    ensure_eq(&u64::try_from(out.len())?, &written, "返回值等于写出字节数")?;
    ensure(out.len().is_multiple_of(4), "段尾对齐")?;
    ensure(out.windows(5).any(|window| window == b"hello"), "内容")?;
    let tail = trailer()?;
    ensure(
        tail.windows(10).any(|window| window == b"TRAILER!!!"),
        "结束标记",
    )?;
    ensure_eq(&pad4(5), &3, "pad4(5)")?;
    ensure_eq(&pad4(8), &0, "pad4(8)")?;
    let mut short: &[u8] = b"abc";
    ensure(
        append_file(&mut Vec::new(), "b", 5, &mut short, 1).is_err(),
        "内容不足时报错",
    )?;
    Ok(())
}
