//! `answer` 模块单元测试：领域要约编码、目的地址、应答类型与请求视图（含畸形选项）。

use std::net::{Ipv4Addr, SocketAddrV4};

use xpxe_domain::{
    BootOffer, BootTarget, FirmwareName, FirmwareSet, HttpUrl, SessionId, Sha256Hex,
};
use xpxe_test_support::{TestResult, ensure, ensure_eq};

use super::{ReplyContext, encode_offer, reply_destination, reply_kind, request_view};
use crate::proxy::ReplyKind;
use crate::wire::{Message, Op, code};

/// 构造请求报文。
fn request(message_type: u8, options: Vec<(u8, Vec<u8>)>) -> Message {
    let mut chaddr = [0_u8; 16];
    for (slot, byte) in chaddr.iter_mut().zip([0x52, 0x54, 0, 0x77, 1, 0xb1]) {
        *slot = byte;
    }
    let mut all = vec![(code::MESSAGE_TYPE, vec![message_type])];
    all.extend(options);
    Message {
        op: Op::Request,
        htype: 1,
        hlen: 6,
        hops: 0,
        xid: 0x1234,
        secs: 0,
        flags: 0,
        ciaddr: Ipv4Addr::UNSPECIFIED,
        yiaddr: Ipv4Addr::UNSPECIFIED,
        siaddr: Ipv4Addr::UNSPECIFIED,
        giaddr: Ipv4Addr::UNSPECIFIED,
        chaddr,
        sname: Vec::new(),
        file: Vec::new(),
        options: all,
    }
}

/// 测试固件集。
///
/// # Errors
///
/// 摘要解析失败。
fn firmware() -> TestResult<FirmwareSet> {
    Ok(FirmwareSet {
        undionly: Sha256Hex::parse(&"1".repeat(64))?,
        ipxe_efi: Sha256Hex::parse(&"2".repeat(64))?,
        snponly: Sha256Hex::parse(&"3".repeat(64))?,
    })
}

/// 三类要约的编码：TFTP、iPXE 脚本、HTTP Boot，以及 66/67 镜像。
///
/// # Errors
///
/// 断言失败。
#[test]
fn encodes_offers() -> TestResult {
    let set = firmware()?;
    let server = Ipv4Addr::new(10, 77, 0, 1);
    let req = request(1, vec![(code::CLIENT_UUID, vec![0, 1, 2])]);
    let session = SessionId::parse("s1")?;
    let mac = xpxe_domain::MacAddr::parse("52:54:00:77:01:b1")?;
    let ctx = ReplyContext {
        server,
        kind: ReplyKind::Offer,
        firmware: &set,
        mirror_bootfile: true,
    };
    let tftp = BootOffer {
        mac,
        session,
        target: BootTarget::Tftp(FirmwareName::IpxeEfi),
        http_client_class: false,
    };
    let reply = encode_offer(&req, &tftp, &ctx);
    ensure_eq(
        &reply.file.as_slice(),
        &b"fw/222222222222/ipxe.efi".as_slice(),
        "file",
    )?;
    ensure_eq(&reply.siaddr, &server, "siaddr")?;
    ensure_eq(&reply.yiaddr, &Ipv4Addr::UNSPECIFIED, "yiaddr = 0")?;
    ensure_eq(
        &reply.option(code::VENDOR_SPECIFIC),
        &Some([6_u8, 1, 8, 255].as_slice()),
        "Opt43",
    )?;
    ensure_eq(
        &reply.option(code::TFTP_SERVER),
        &Some(b"10.77.0.1".as_slice()),
        "Opt66",
    )?;
    ensure_eq(
        &reply.option(code::BOOTFILE),
        &Some(b"fw/222222222222/ipxe.efi".as_slice()),
        "Opt67",
    )?;
    ensure_eq(
        &reply.option(code::CLIENT_UUID),
        &Some([0_u8, 1, 2].as_slice()),
        "Opt97 回显",
    )?;
    Ok(())
}

/// iPXE 脚本要约：`siaddr` 非零、无 Opt43、未开镜像不写 67。
///
/// # Errors
///
/// 断言失败。
#[test]
fn encodes_ipxe_script() -> TestResult {
    let set = firmware()?;
    let server = Ipv4Addr::new(10, 77, 0, 1);
    let req = request(1, Vec::new());
    let url = HttpUrl::parse("http://10.77.0.1:8080/p/tok/boot.ipxe")?;
    let script = BootOffer {
        mac: xpxe_domain::MacAddr::parse("52:54:00:77:01:b1")?,
        session: SessionId::parse("s1")?,
        target: BootTarget::Http(url),
        http_client_class: false,
    };
    let plain = ReplyContext {
        server,
        kind: ReplyKind::Offer,
        firmware: &set,
        mirror_bootfile: false,
    };
    let ipxe = encode_offer(&req, &script, &plain);
    ensure_eq(&ipxe.siaddr, &server, "iPXE 要求 siaddr 非零")?;
    ensure_eq(&ipxe.option(code::VENDOR_SPECIFIC), &None, "iPXE 无 Opt43")?;
    ensure_eq(
        &ipxe.option(code::VENDOR_CLASS),
        &Some(b"PXEClient".as_slice()),
        "Opt60",
    )?;
    ensure_eq(&ipxe.option(code::BOOTFILE), &None, "未开镜像不写 67")?;
    Ok(())
}

/// HTTP Boot 要约：`HTTPClient`、`siaddr = 0`、不写 66。
///
/// # Errors
///
/// 断言失败。
#[test]
fn encodes_http_boot() -> TestResult {
    let set = firmware()?;
    let server = Ipv4Addr::new(10, 77, 0, 1);
    let req = request(1, Vec::new());
    let plain = ReplyContext {
        server,
        kind: ReplyKind::Offer,
        firmware: &set,
        mirror_bootfile: true,
    };
    let mac = xpxe_domain::MacAddr::parse("52:54:00:77:01:b1")?;
    let session = SessionId::parse("s1")?;
    let http = BootOffer {
        mac,
        session,
        target: BootTarget::Http(HttpUrl::parse("http://10.77.0.1:8080/fw/x64/ipxe.efi")?),
        http_client_class: true,
    };
    let reply = encode_offer(&req, &http, &plain);
    ensure_eq(
        &reply.option(code::VENDOR_CLASS),
        &Some(b"HTTPClient".as_slice()),
        "HTTPClient",
    )?;
    ensure_eq(
        &reply.siaddr,
        &Ipv4Addr::UNSPECIFIED,
        "HTTP Boot 不写 siaddr",
    )?;
    Ok(())
}

/// 目的地址与应答类型。
///
/// # Errors
///
/// 断言失败。
#[test]
fn destinations_and_kinds() -> TestResult {
    let from = SocketAddrV4::new(Ipv4Addr::new(10, 77, 0, 50), 68);
    let mut req = request(1, Vec::new());
    ensure_eq(
        &reply_destination(&req, 67, from),
        &SocketAddrV4::new(Ipv4Addr::BROADCAST, 68),
        "ciaddr=0 广播",
    )?;
    req.ciaddr = Ipv4Addr::new(10, 77, 0, 50);
    ensure_eq(&reply_destination(&req, 67, from), &from, "ciaddr 单播")?;
    req.flags = 0x8000;
    ensure_eq(
        &reply_destination(&req, 67, from),
        &SocketAddrV4::new(Ipv4Addr::BROADCAST, 68),
        "广播标志",
    )?;
    let pxe_from = SocketAddrV4::new(Ipv4Addr::new(10, 77, 0, 50), 4011);
    ensure_eq(
        &reply_destination(&req, 4011, pxe_from),
        &pxe_from,
        "4011 回源",
    )?;

    ensure_eq(
        &reply_kind(&request(1, Vec::new()), 67),
        &Some(ReplyKind::Offer),
        "67 DISCOVER",
    )?;
    ensure_eq(
        &reply_kind(&request(3, Vec::new()), 4011),
        &Some(ReplyKind::Ack),
        "4011 REQUEST",
    )?;
    ensure_eq(
        &reply_kind(&request(3, Vec::new()), 67),
        &None,
        "67 REQUEST 不应答",
    )?;
    ensure_eq(
        &reply_kind(&request(8, Vec::new()), 67),
        &None,
        "INFORM 不应答",
    )?;
    ensure_eq(
        &reply_kind(&request(7, Vec::new()), 4011),
        &None,
        "RELEASE 不应答",
    )?;
    let mut reply = request(1, Vec::new());
    reply.op = Op::Reply;
    ensure_eq(&reply_kind(&reply, 67), &None, "BOOTREPLY 不应答")?;
    Ok(())
}

/// 请求视图：缺 Opt93、Opt93 多值、Opt77 大小写、超长 Opt60、截断选项。
///
/// # Errors
///
/// 断言失败。
#[test]
fn request_views() -> TestResult {
    let bare = request(1, vec![(code::VENDOR_CLASS, b"PXEClient".to_vec())]);
    let view = request_view(&bare).ok_or("视图")?;
    ensure_eq(&view.arch, &None, "缺 Opt93")?;
    ensure_eq(&view.mac.to_colon().as_str(), &"52:54:00:77:01:b1", "MAC")?;

    let multi = request(1, vec![(code::CLIENT_ARCH, vec![0, 7, 0, 0])]);
    ensure_eq(
        &request_view(&multi).ok_or("视图")?.arch,
        &Some(7),
        "Opt93 多值取第一个",
    )?;

    let upper = request(1, vec![(code::USER_CLASS, b"IPXE".to_vec())]);
    ensure(
        !request_view(&upper).ok_or("视图")?.is_ipxe(),
        "Opt77 大小写敏感：IPXE 不是 iPXE",
    )?;
    let rfc3004 = request(1, vec![(code::USER_CLASS, b"\x04iPXE".to_vec())]);
    ensure(
        request_view(&rfc3004).ok_or("视图")?.is_ipxe(),
        "RFC 3004 格式",
    )?;

    let long = "PXEClient:".repeat(25);
    let long_req = request(1, vec![(code::VENDOR_CLASS, long.as_bytes().to_vec())]);
    let encoded = long_req.encode()?;
    let decoded = Message::decode(&encoded)?;
    ensure_eq(
        &request_view(&decoded).ok_or("视图")?.vendor_class,
        &Some(long),
        "250 字节 Opt60 往返",
    )?;

    let mut truncated = request(1, Vec::new()).encode()?;
    let _end: Option<u8> = truncated.pop();
    truncated.extend_from_slice(&[code::VENDOR_CLASS, 40, b'P']);
    ensure(Message::decode(&truncated).is_err(), "截断选项解析失败")?;

    let mut no_mac = request(1, Vec::new());
    no_mac.hlen = 0;
    ensure(request_view(&no_mac).is_none(), "无 MAC 无视图")?;
    Ok(())
}
