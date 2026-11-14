//! `wire` 模块单元测试：解析、序列化与边界输入。

use std::net::Ipv4Addr;

use xpxe_test_support::{TestResult, ensure, ensure_eq};

use super::{DecodeError, Message, MessageType, Op, code};

/// 构造一个 EDK2 风格的 PXE DISCOVER 报文。
fn discover() -> Message {
    let mut chaddr = [0_u8; 16];
    chaddr
        .iter_mut()
        .zip([0x52_u8, 0x54, 0x00, 0x77, 0x01, 0x05])
        .for_each(|(slot, byte)| *slot = byte);
    Message {
        op: Op::Request,
        htype: 1,
        hlen: 6,
        hops: 0,
        xid: 0x1234_5678,
        secs: 4,
        flags: 0x8000,
        ciaddr: Ipv4Addr::UNSPECIFIED,
        yiaddr: Ipv4Addr::UNSPECIFIED,
        siaddr: Ipv4Addr::UNSPECIFIED,
        giaddr: Ipv4Addr::UNSPECIFIED,
        chaddr,
        sname: Vec::new(),
        file: Vec::new(),
        options: vec![
            (code::MESSAGE_TYPE, vec![1]),
            (code::CLIENT_ARCH, vec![0, 7]),
            (
                code::VENDOR_CLASS,
                b"PXEClient:Arch:00007:UNDI:003016".to_vec(),
            ),
            (code::CLIENT_UUID, vec![0; 17]),
        ],
    }
}

/// 序列化后再解析，得到完全相同的报文。
///
/// # Errors
///
/// 往返不一致时返回错误。
#[test]
fn roundtrip_preserves_everything() -> TestResult {
    let original = discover();
    let bytes = original.encode()?;
    ensure_eq(&bytes.len(), &(236 + 4 + 3 + 4 + 34 + 19 + 1), "报文长度")?;
    let decoded = Message::decode(&bytes)?;
    ensure_eq(&decoded, &original, "往返必须一致")?;
    Ok(())
}

/// 访问器返回类型、MAC、架构与厂商类别。
///
/// # Errors
///
/// 字段不符时返回错误。
#[test]
fn accessors_read_pxe_fields() -> TestResult {
    let message = discover();
    ensure_eq(
        &message.message_type(),
        &Some(MessageType::Discover),
        "报文类型",
    )?;
    ensure_eq(
        &message.mac(),
        &Some([0x52, 0x54, 0x00, 0x77, 0x01, 0x05]),
        "MAC",
    )?;
    ensure_eq(&message.client_arch(), &Some(7_u16), "架构")?;
    ensure(
        message
            .vendor_class()
            .is_some_and(|class| class.starts_with("PXEClient")),
        "厂商类别",
    )?;
    ensure(!message.is_ipxe(), "EDK2 不是 iPXE")?;
    Ok(())
}

/// iPXE 识别：选项 77 的两种编码与选项 175 任一即可。
///
/// # Errors
///
/// 识别结果不符时返回错误。
#[test]
fn ipxe_detected_by_either_option() -> TestResult {
    let mut raw = discover();
    raw.options.push((code::USER_CLASS, b"iPXE".to_vec()));
    ensure(raw.is_ipxe(), "裸字符串 77")?;
    let mut prefixed = discover();
    prefixed
        .options
        .push((code::USER_CLASS, b"\x04iPXE".to_vec()));
    ensure(prefixed.is_ipxe(), "RFC 3004 长度前缀 77")?;
    let mut encap = discover();
    encap.options.push((code::IPXE_ENCAP, vec![1, 1, 1]));
    ensure(encap.is_ipxe(), "只有 175")?;
    Ok(())
}

/// 截断、超长、魔数错误、缺少结束选项都被拒绝。
///
/// # Errors
///
/// 某个畸形输入未被拒绝时返回错误。
#[test]
fn malformed_inputs_rejected() -> TestResult {
    let bytes = discover().encode()?;
    ensure_eq(
        &Message::decode(bytes.get(..100).unwrap_or_default()),
        &Err(DecodeError::TooShort),
        "过短",
    )?;
    let mut no_end = bytes.clone();
    let _end: Option<u8> = no_end.pop();
    ensure_eq(
        &Message::decode(&no_end),
        &Err(DecodeError::MissingEnd),
        "缺结束",
    )?;
    let mut truncated = bytes.get(..240).unwrap_or_default().to_vec();
    truncated.extend_from_slice(&[code::VENDOR_CLASS, 40, b'P']);
    ensure_eq(
        &Message::decode(&truncated),
        &Err(DecodeError::TruncatedOption(code::VENDOR_CLASS)),
        "选项越界",
    )?;
    let mut bad_magic = bytes;
    if let Some(byte) = bad_magic.get_mut(236) {
        *byte = 0;
    }
    ensure_eq(
        &Message::decode(&bad_magic),
        &Err(DecodeError::BadMagic),
        "魔数",
    )?;
    ensure_eq(
        &Message::decode(&vec![1_u8; 1501]),
        &Err(DecodeError::TooLong),
        "超长",
    )?;
    Ok(())
}

/// 任意截断前缀都不会让解析器崩溃，只会返回错误或成功。
///
/// # Errors
///
/// 截断后的完整报文以外的前缀被接受时返回错误。
#[test]
fn every_prefix_is_handled() -> TestResult {
    let bytes = discover().encode()?;
    for len in 0..bytes.len() {
        let prefix = bytes.get(..len).unwrap_or_default();
        ensure(Message::decode(prefix).is_err(), "不完整报文必须被拒绝")?;
    }
    Ok(())
}

/// 超过 255 字节的选项无法序列化。
///
/// # Errors
///
/// 超长选项被接受时返回错误。
#[test]
fn oversized_option_rejected() -> TestResult {
    let mut message = discover();
    message.options.push((code::BOOTFILE, vec![b'a'; 256]));
    ensure(message.encode().is_err(), "超长选项必须失败")?;
    Ok(())
}

/// 实验室抓到（及构造）的全部夹具都能解码、往返一致，且 iPXE 判定与描述文件一致。
///
/// # Errors
///
/// 任一夹具不满足时返回错误。
#[test]
fn captured_fixtures_round_trip() -> TestResult {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut count = 0_usize;
    for entry in std::fs::read_dir(&dir)? {
        let path = entry?.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("bin") {
            continue;
        }
        let name = path.display().to_string();
        let bytes = std::fs::read(&path)?;
        let message = Message::decode(&bytes)?;
        ensure_eq(&Message::decode(&message.encode()?)?, &message, &name)?;
        let description = std::fs::read_to_string(path.with_extension("json"))?;
        if description.contains("\"is_ipxe\"") {
            let expected = description.contains("\"is_ipxe\": true");
            ensure_eq(&message.is_ipxe(), &expected, &name)?;
        }
        count = count.saturating_add(1);
    }
    ensure(count >= 12, "夹具数量不足 12 个")?;
    Ok(())
}
