//! 签名包解析单元测试。

use xpxe_test_support::{TestResult, ensure, ensure_eq};

use super::{left_pad, parse_signatures, subpacket_len};
use crate::packet::Reader;

/// 子包长度三种编码。
///
/// # Errors
///
/// 结果不符时返回错误。
#[test]
fn subpacket_lengths() -> TestResult {
    ensure_eq(&subpacket_len(&mut Reader::new(&[5]))?, &5, "one byte")?;
    ensure_eq(
        &subpacket_len(&mut Reader::new(&[192, 0]))?,
        &192,
        "two bytes",
    )?;
    ensure_eq(
        &subpacket_len(&mut Reader::new(&[255, 0, 0, 1, 0]))?,
        &256,
        "five bytes",
    )?;
    ensure(
        subpacket_len(&mut Reader::new(&[200])).is_err(),
        "truncated",
    )?;
    Ok(())
}

/// 左补零与超长拒绝。
///
/// # Errors
///
/// 结果不符时返回错误。
#[test]
fn padding() -> TestResult {
    ensure_eq(&left_pad::<4>(&[1, 2])?, &[0, 0, 1, 2], "pad")?;
    ensure(left_pad::<1>(&[1, 2]).is_err(), "too long")?;
    Ok(())
}

/// SHA-1 签名与非签名报文被拒。
///
/// # Errors
///
/// 结果不符时返回错误。
#[test]
fn rejects_sha1_and_empty() -> TestResult {
    // 新格式 tag 2，长度 10：v4, type 0, RSA, SHA-1(2), hashed 0, unhashed 0, left16, 空 MPI
    let sha1 = [0xC2, 10, 4, 0, 1, 2, 0, 0, 0, 0, 0xAB, 0xCD];
    let error = parse_signatures(&sha1).err();
    ensure_eq(
        &error.map(|found| found.code()),
        &Some("unsupported_hash"),
        "sha1",
    )?;
    // tag 11（字面数据）不是签名
    let literal = [0xCB, 1, 0];
    ensure(parse_signatures(&literal).is_err(), "no signature")?;
    Ok(())
}
