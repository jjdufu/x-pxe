//! `proxy` 模块单元测试：应答报文的关键字段。

use std::net::Ipv4Addr;

use xpxe_test_support::{TestResult, ensure, ensure_eq};

use super::{BootTarget, ReplyKind, build_reply};
use crate::wire::{Message, MessageType, Op, code};

/// 一个带 GUID 的 PXE DISCOVER。
fn request() -> Message {
    Message {
        op: Op::Request,
        htype: 1,
        hlen: 6,
        hops: 0,
        xid: 7,
        secs: 0,
        flags: 0x8000,
        ciaddr: Ipv4Addr::UNSPECIFIED,
        yiaddr: Ipv4Addr::UNSPECIFIED,
        siaddr: Ipv4Addr::UNSPECIFIED,
        giaddr: Ipv4Addr::UNSPECIFIED,
        chaddr: [1; 16],
        sname: Vec::new(),
        file: Vec::new(),
        options: vec![
            (code::MESSAGE_TYPE, vec![1]),
            (code::CLIENT_UUID, vec![0, 9, 9]),
        ],
    }
}

/// TFTP 应答：PXEClient、siaddr、文件名、GUID 回显、跳过发现，且不分配地址。
///
/// # Errors
///
/// 字段不符时返回错误。
#[test]
fn tftp_offer_fields() -> TestResult {
    let server = Ipv4Addr::new(10, 77, 0, 1);
    let reply = build_reply(
        &request(),
        server,
        ReplyKind::Offer,
        &BootTarget::Tftp {
            server,
            file: "ipxe.efi".to_owned(),
        },
    );
    ensure_eq(&reply.message_type(), &Some(MessageType::Offer), "类型")?;
    ensure_eq(&reply.yiaddr, &Ipv4Addr::UNSPECIFIED, "不分配地址")?;
    ensure_eq(&reply.siaddr, &server, "siaddr")?;
    ensure_eq(&reply.file.as_slice(), &b"ipxe.efi".as_slice(), "file")?;
    ensure_eq(
        &reply.option(code::CLIENT_UUID),
        &Some([0_u8, 9, 9].as_slice()),
        "GUID 回显",
    )?;
    ensure_eq(
        &reply.option(code::VENDOR_CLASS),
        &Some(b"PXEClient".as_slice()),
        "厂商类别",
    )?;
    ensure(reply.option(code::VENDOR_SPECIFIC).is_some(), "选项 43")?;
    ensure_eq(&reply.xid, &7_u32, "xid")?;
    ensure(reply.encode().is_ok(), "可序列化")?;
    Ok(())
}

/// HTTP Boot 应答回 `HTTPClient`，`file` 为带端口的 URL，不带选项 43。
///
/// # Errors
///
/// 字段不符时返回错误。
#[test]
fn http_boot_offer_fields() -> TestResult {
    let reply = build_reply(
        &request(),
        Ipv4Addr::new(10, 77, 0, 1),
        ReplyKind::Ack,
        &BootTarget::HttpBoot {
            url: "http://10.77.0.1:8080/fw/x64/ipxe.efi".to_owned(),
        },
    );
    ensure_eq(&reply.message_type(), &Some(MessageType::Ack), "类型")?;
    ensure_eq(
        &reply.option(code::VENDOR_CLASS),
        &Some(b"HTTPClient".as_slice()),
        "厂商类别",
    )?;
    ensure(
        reply.option(code::VENDOR_SPECIFIC).is_none(),
        "HTTP Boot 不带 43",
    )?;
    ensure_eq(
        &reply.file.as_slice(),
        &b"http://10.77.0.1:8080/fw/x64/ipxe.efi".as_slice(),
        "URL",
    )?;
    Ok(())
}

/// iPXE 脚本应答：`file` 为脚本 URL，`siaddr` 非零（iPXE 接受 `ProxyDHCP` 的前提）。
///
/// # Errors
///
/// 字段不符时返回错误。
#[test]
fn ipxe_script_reply_sets_siaddr() -> TestResult {
    let server = Ipv4Addr::new(10, 77, 0, 1);
    let reply = build_reply(
        &request(),
        server,
        ReplyKind::Ack,
        &BootTarget::IpxeScript {
            url: "http://10.77.0.1:8080/boot.ipxe".to_owned(),
        },
    );
    ensure_eq(&reply.siaddr, &server, "siaddr")?;
    ensure_eq(
        &reply.file.as_slice(),
        &b"http://10.77.0.1:8080/boot.ipxe".as_slice(),
        "脚本 URL",
    )?;
    ensure(
        reply.option(code::VENDOR_SPECIFIC).is_none(),
        "iPXE 脚本不带 43",
    )?;
    Ok(())
}
