//! 构造报文测试。

use std::net::Ipv4Addr;

use xpxe_dhcp::wire::{Message, MessageType, code};
use xpxe_test_support::{TestResult, ensure, ensure_eq};

use super::{Client, Identity, carries_boot, client_message};

/// 测试身份。
const fn identity(mac: [u8; 6]) -> Identity {
    Identity {
        mac,
        xid: 9,
        guid: [7; 16],
        address: Ipv4Addr::UNSPECIFIED,
    }
}

/// BIOS 非 iPXE DISCOVER：Opt93=0、无 Opt77/175，可编解码往返。
///
/// # Errors
///
/// 字段不符时返回错误。
#[test]
fn legacy_discover_fields() -> TestResult {
    let message = client_message(
        Client::Legacy,
        MessageType::Discover,
        identity([2, 0, 0, 0, 0, 1]),
    );
    let decoded = Message::decode(&message.encode()?)?;
    ensure_eq(&decoded.client_arch(), &Some(0), "架构")?;
    ensure(!decoded.is_ipxe(), "非 iPXE")?;
    ensure(
        decoded
            .vendor_class()
            .is_some_and(|value| value.starts_with("PXEClient:Arch:00000")),
        "厂商类别",
    )?;
    ensure_eq(
        &decoded.option(code::CLIENT_UUID).map(<[u8]>::len),
        &Some(17),
        "GUID",
    )?;
    let plain = client_message(Client::Plain, MessageType::Discover, identity([2; 6]));
    ensure_eq(&plain.vendor_class(), &None, "普通客户端无 Opt60")?;
    Ok(())
}

/// 引导参数判定。
///
/// # Errors
///
/// 判定不符时返回错误。
#[test]
fn boot_params_detection() -> TestResult {
    let mut reply = client_message(Client::Plain, MessageType::Offer, identity([2; 6]));
    ensure(!carries_boot(&reply), "无引导参数")?;
    reply.file = b"pxelinux.0".to_vec();
    ensure(carries_boot(&reply), "file")?;
    Ok(())
}
