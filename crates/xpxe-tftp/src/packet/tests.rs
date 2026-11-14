//! 报文编解码测试。

use xpxe_test_support::{TestResult, ensure, ensure_eq};

use super::{ErrorCode, Packet, ParseError};

/// iPXE 风格的读请求：选项键大小写不敏感。
///
/// # Errors
///
/// 解析结果不符时返回错误。
#[test]
fn parses_read_request_with_options() -> TestResult {
    let raw = b"\x00\x01ipxe.efi\x00octet\x00BLKSIZE\x001432\x00tsize\x000\x00";
    let packet = Packet::parse(raw)?;
    ensure_eq(
        &packet,
        &Packet::Read {
            filename: "ipxe.efi".to_owned(),
            mode: "octet".to_owned(),
            options: vec![
                ("blksize".to_owned(), "1432".to_owned()),
                ("tsize".to_owned(), "0".to_owned()),
            ],
        },
        "rrq",
    )?;
    ensure_eq(
        &Packet::parse(&packet.encode())?,
        &Packet::Read {
            filename: "ipxe.efi".to_owned(),
            mode: "octet".to_owned(),
            options: vec![
                ("blksize".to_owned(), "1432".to_owned()),
                ("tsize".to_owned(), "0".to_owned()),
            ],
        },
        "往返",
    )?;
    Ok(())
}

/// 各类报文往返一致。
///
/// # Errors
///
/// 往返不一致时返回错误。
#[test]
fn round_trips() -> TestResult {
    let packets = [
        Packet::Data {
            block: 65535,
            data: vec![1, 2, 3],
        },
        Packet::Ack { block: 7 },
        Packet::Error {
            code: ErrorCode::FileNotFound,
            message: "nope".to_owned(),
        },
        Packet::OptionAck {
            options: vec![("blksize".to_owned(), "1468".to_owned())],
        },
    ];
    for packet in packets {
        ensure_eq(&Packet::parse(&packet.encode())?, &packet, "往返")?;
    }
    Ok(())
}

/// 畸形输入被拒绝。
///
/// # Errors
///
/// 畸形输入被接受时返回错误。
#[test]
fn rejects_malformed() -> TestResult {
    ensure_eq(&Packet::parse(&[0]), &Err(ParseError::TooShort), "过短")?;
    ensure_eq(
        &Packet::parse(&[0, 9]),
        &Err(ParseError::UnknownOpcode(9)),
        "未知",
    )?;
    ensure_eq(
        &Packet::parse(b"\x00\x01file"),
        &Err(ParseError::BadString),
        "无 NUL",
    )?;
    ensure(
        Packet::parse(b"\x00\x01\xff\x00octet\x00").is_err(),
        "非 UTF-8",
    )?;
    ensure_eq(
        &ErrorCode::from_u16(42),
        &ErrorCode::Undefined,
        "未知错误码",
    )?;
    Ok(())
}
