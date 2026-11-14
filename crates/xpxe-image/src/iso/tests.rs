//! ISO 写出器测试：卷描述符、El Torito 引导目录与文件放置。

use xpxe_test_support::{TestResult, ensure, ensure_eq};

use super::{Entry, SECTOR, build};
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

/// 读取 `image` 中第 `lba` 个扇区偏移 `offset` 起的 `len` 字节。
fn at(image: &[u8], lba: usize, offset: usize, len: usize) -> Option<&[u8]> {
    let start = lba.checked_mul(SECTOR)?.checked_add(offset)?;
    image.get(start..start.checked_add(len)?)
}

/// 主卷描述符、引导记录、终止符与引导目录都在约定位置，且验证项校验和为零。
///
/// # Errors
///
/// 结构不符时返回错误。
#[test]
fn descriptors_and_catalog() -> TestResult {
    let esp = vec![7_u8; 3000];
    let image = build(
        &[Entry {
            name: "ESP.IMG",
            data: &esp,
        }],
        "ESP.IMG",
        "xpxe-test",
        STAMP,
    )?;
    ensure_eq(&at(&image, 16, 0, 6), &Some(b"\x01CD001".as_slice()), "PVD")?;
    ensure_eq(
        &at(&image, 16, 40, 9),
        &Some(b"XPXE-TEST".as_slice()),
        "卷标大写",
    )?;
    ensure_eq(
        &at(&image, 17, 7, 23),
        &Some(b"EL TORITO SPECIFICATION".as_slice()),
        "引导记录",
    )?;
    ensure_eq(
        &at(&image, 18, 0, 6),
        &Some(b"\xffCD001".as_slice()),
        "终止符",
    )?;
    Ok(())
}

/// El Torito 验证项校验和为零，默认项指向 ESP。
///
/// # Errors
///
/// 结构不符时返回错误。
#[test]
fn catalog_points_at_esp() -> TestResult {
    let esp = vec![7_u8; 3000];
    let image = build(
        &[Entry {
            name: "ESP.IMG",
            data: &esp,
        }],
        "ESP.IMG",
        "xpxe-test",
        STAMP,
    )?;
    let validation = at(&image, 19, 0, 32).unwrap_or_default();
    ensure_eq(
        &validation.get(..2),
        &Some([1_u8, 0xEF].as_slice()),
        "平台 0xEF",
    )?;
    let sum = validation
        .as_chunks::<2>()
        .0
        .iter()
        .copied()
        .fold(0_u16, |acc, pair| {
            acc.wrapping_add(u16::from_le_bytes(pair))
        });
    ensure_eq(&sum, &0_u16, "验证项校验和")?;
    ensure_eq(
        &at(&image, 19, 32, 2),
        &Some([0x88_u8, 0].as_slice()),
        "可引导 + no-emulation",
    )?;
    ensure_eq(
        &at(&image, 19, 38, 2),
        &Some(6_u16.to_le_bytes().as_slice()),
        "512 字节扇区数",
    )?;
    ensure_eq(
        &at(&image, 19, 40, 4),
        &Some(23_u32.to_le_bytes().as_slice()),
        "ESP 位置",
    )?;
    ensure_eq(
        &at(&image, 23, 0, 3),
        &Some([7_u8, 7, 7].as_slice()),
        "ESP 内容",
    )?;
    ensure_eq(&image.len(), &(25 * SECTOR), "总大小")?;
    Ok(())
}

/// 文件名非法、重名或 ESP 不存在时失败。
///
/// # Errors
///
/// 非法输入被接受时返回错误。
#[test]
fn invalid_inputs_rejected() -> TestResult {
    let esp = Entry {
        name: "ESP.IMG",
        data: b"x",
    };
    ensure(build(&[esp], "NOPE.IMG", "v", STAMP).is_err(), "ESP 不存在")?;
    ensure(build(&[esp, esp], "ESP.IMG", "v", STAMP).is_err(), "重名")?;
    ensure(
        build(
            &[Entry {
                name: "esp.img",
                data: b"x",
            }],
            "esp.img",
            "v",
            STAMP,
        )
        .is_err(),
        "小写名",
    )?;
    Ok(())
}

/// 相同输入得到相同字节。
///
/// # Errors
///
/// 两次结果不同时返回错误。
#[test]
fn deterministic() -> TestResult {
    let files = [
        Entry {
            name: "ESP.IMG",
            data: b"abc",
        },
        Entry {
            name: "README.TXT",
            data: b"r",
        },
    ];
    ensure_eq(
        &build(&files, "ESP.IMG", "v", STAMP)?,
        &build(&files, "ESP.IMG", "v", STAMP)?,
        "确定性",
    )?;
    Ok(())
}
