//! `probe` 模块单元测试：探测报文与三类应答者分类。

use std::net::Ipv4Addr;

use xpxe_domain::ResponderClass;
use xpxe_test_support::{TestResult, ensure, ensure_eq};

use super::{classify, discover_packet, local_admin_mac};
use crate::wire::{Message, MessageType, Op, code};

/// 构造一个 OFFER。
fn offer(xid: u32, yiaddr: [u8; 4], options: Vec<(u8, Vec<u8>)>) -> Message {
    let mut reply = discover_packet([2, 0, 0, 0, 0, 1], xid);
    reply.op = Op::Reply;
    reply.yiaddr = Ipv4Addr::from(yiaddr);
    reply.options = vec![(code::MESSAGE_TYPE, vec![MessageType::Offer.to_byte()])];
    reply.options.extend(options);
    reply
}

/// 探测 DISCOVER 的形状与本地管理 MAC。
///
/// # Errors
///
/// 断言失败。
#[test]
fn discover_shape() -> TestResult {
    let mac = local_admin_mac([0xff, 1, 2, 3, 4, 5]);
    ensure_eq(&mac, &[0xfe, 1, 2, 3, 4, 5], "本地管理、单播")?;
    let message = discover_packet(mac, 9);
    ensure_eq(
        &message.message_type(),
        &Some(MessageType::Discover),
        "DISCOVER",
    )?;
    ensure(
        message
            .vendor_class()
            .is_some_and(|vendor| vendor.starts_with("PXEClient")),
        "PXEClient 厂商类",
    )?;
    ensure_eq(&message.mac(), &Some(mac), "chaddr")?;
    let encoded = message.encode()?;
    ensure_eq(&Message::decode(&encoded)?, &message, "往返")?;
    Ok(())
}

/// 三类应答者与无关报文。
///
/// # Errors
///
/// 断言失败。
#[test]
fn classifies() -> TestResult {
    let source = Ipv4Addr::new(10, 77, 0, 9);
    let plain = offer(
        7,
        [10, 77, 0, 100],
        vec![(code::SERVER_ID, vec![10, 77, 0, 2])],
    );
    let found = classify(&plain, 7, source).ok_or("应分类")?;
    ensure_eq(&found.class, &ResponderClass::AddressOnly, "普通 DHCP")?;
    ensure_eq(&found.server, &Ipv4Addr::new(10, 77, 0, 2), "取选项 54")?;

    let mut dnsmasq = offer(7, [10, 77, 0, 100], Vec::new());
    dnsmasq.siaddr = source;
    let quiet = classify(&dnsmasq, 7, source).ok_or("应分类")?;
    ensure_eq(
        &quiet.class,
        &ResponderClass::AddressOnly,
        "只有 siaddr 不算引导参数",
    )?;
    ensure_eq(&quiet.server, &source, "无选项 54 取源地址")?;

    let mut rogue = offer(
        7,
        [10, 77, 0, 101],
        vec![(code::BOOTFILE, b"evil.efi".to_vec())],
    );
    rogue.siaddr = Ipv4Addr::new(10, 77, 0, 3);
    let bad = classify(&rogue, 7, source).ok_or("应分类")?;
    ensure_eq(&bad.class, &ResponderClass::PxeCapable, "下发引导文件")?;
    ensure_eq(&bad.file.as_deref(), &Some("evil.efi"), "选项 67")?;

    let mut with_file = offer(7, [10, 77, 0, 101], Vec::new());
    with_file.file = b"pxelinux.0".to_vec();
    ensure_eq(
        &classify(&with_file, 7, source).map(|item| item.class),
        &Some(ResponderClass::PxeCapable),
        "file 字段",
    )?;

    let proxy = offer(
        7,
        [0, 0, 0, 0],
        vec![(code::VENDOR_CLASS, b"PXEClient".to_vec())],
    );
    ensure_eq(
        &classify(&proxy, 7, source).map(|item| item.class),
        &Some(ResponderClass::Proxy),
        "ProxyDHCP",
    )?;
    ensure(classify(&proxy, 8, source).is_none(), "xid 不符忽略")?;
    let mut ack = plain;
    ack.options = vec![(code::MESSAGE_TYPE, vec![MessageType::Ack.to_byte()])];
    ensure(classify(&ack, 7, source).is_none(), "只看 OFFER")?;
    Ok(())
}
