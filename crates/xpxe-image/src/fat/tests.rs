//! FAT 写出器测试：引导扇区、变体选择、长文件名与确定性。

use xpxe_test_support::{TestResult, ensure, ensure_eq};

use super::{FileSpec, Variant, build, checksum, short_alias, short_name_exact};
use crate::Timestamp;

/// 固定的测试时间。
const STAMP: Timestamp = Timestamp {
    year: 2026,
    month: 11,
    day: 14,
    hour: 9,
    minute: 30,
    second: 0,
};

/// 小镜像选择 FAT12，引导扇区签名、类型字符串与卷标正确，长度是扇区整数倍。
///
/// # Errors
///
/// 结构不符时返回错误。
#[test]
fn small_image_is_fat12() -> TestResult {
    let efi = vec![0xAB_u8; 1_000_000];
    let files = [
        FileSpec {
            path: "EFI/BOOT/BOOTX64.EFI",
            data: &efi,
        },
        FileSpec {
            path: "EFI/BOOT/autoexec.ipxe",
            data: b"#!ipxe\nshell\n",
        },
    ];
    let image = build(&files, "XPXE-ESP", STAMP)?;
    ensure_eq(&image.variant, &Variant::Fat12, "变体")?;
    ensure(image.bytes.len().is_multiple_of(512), "扇区对齐")?;
    ensure_eq(
        &image.bytes.get(510..512),
        &Some([0x55_u8, 0xAA].as_slice()),
        "引导签名",
    )?;
    ensure_eq(
        &image.bytes.get(54..62),
        &Some(b"FAT12   ".as_slice()),
        "类型字符串",
    )?;
    ensure_eq(
        &image.bytes.get(43..54),
        &Some(b"XPXE-ESP   ".as_slice()),
        "卷标",
    )?;
    ensure(
        image
            .bytes
            .windows(4)
            .any(|window| window == [0xAB, 0xAB, 0xAB, 0xAB]),
        "文件内容写入",
    )?;
    Ok(())
}

/// 相同输入得到相同字节。
///
/// # Errors
///
/// 两次结果不同时返回错误。
#[test]
fn output_is_deterministic() -> TestResult {
    let files = [FileSpec {
        path: "A/B.TXT",
        data: b"hello",
    }];
    let first = build(&files, "X", STAMP)?;
    let second = build(&files, "X", STAMP)?;
    ensure_eq(&first, &second, "确定性")?;
    Ok(())
}

/// 超过 FAT12 上限的内容切换到 FAT16。
///
/// # Errors
///
/// 变体不符时返回错误。
#[test]
fn large_image_switches_to_fat16() -> TestResult {
    let big = vec![1_u8; 200 * 1024 * 1024];
    let image = build(
        &[FileSpec {
            path: "BIG.BIN",
            data: &big,
        }],
        "BIG",
        STAMP,
    )?;
    ensure_eq(&image.variant, &Variant::Fat16, "变体")?;
    ensure_eq(
        &image.bytes.get(54..62),
        &Some(b"FAT16   ".as_slice()),
        "类型字符串",
    )?;
    Ok(())
}

/// 8.3 判断、短名别名与 VFAT 校验和。
///
/// # Errors
///
/// 结果不符时返回错误。
#[test]
fn short_names() -> TestResult {
    ensure_eq(
        &short_name_exact("BOOTX64.EFI"),
        &Some(*b"BOOTX64 EFI"),
        "合法 8.3",
    )?;
    ensure(
        short_name_exact("autoexec.ipxe").is_none(),
        "小写与 4 位扩展名需要长文件名",
    )?;
    ensure_eq(&short_alias("autoexec.ipxe", 1), b"AUTOEX~1IPX", "短名别名")?;
    ensure_eq(
        &checksum(b"AUTOEX~1IPX"),
        &checksum(b"AUTOEX~1IPX"),
        "校验和稳定",
    )?;
    ensure(
        checksum(b"AUTOEX~1IPX") != checksum(b"AUTOEX~2IPX"),
        "校验和区分名字",
    )?;
    Ok(())
}

/// 重名、文件与目录冲突、空路径都被拒绝。
///
/// # Errors
///
/// 非法输入被接受时返回错误。
#[test]
fn bad_paths_rejected() -> TestResult {
    let dup = [
        FileSpec {
            path: "A.TXT",
            data: b"",
        },
        FileSpec {
            path: "A.TXT",
            data: b"",
        },
    ];
    ensure(build(&dup, "X", STAMP).is_err(), "重名")?;
    let clash = [
        FileSpec {
            path: "A",
            data: b"",
        },
        FileSpec {
            path: "A/B",
            data: b"",
        },
    ];
    ensure(build(&clash, "X", STAMP).is_err(), "文件与目录冲突")?;
    ensure(
        build(
            &[FileSpec {
                path: "A//B",
                data: b"",
            }],
            "X",
            STAMP,
        )
        .is_err(),
        "空路径段",
    )?;
    Ok(())
}
