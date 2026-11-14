//! 引导决策测试。

use std::collections::BTreeMap;
use std::net::Ipv4Addr;

use xpxe_dhcp::proxy::{BootTarget, ReplyKind};
use xpxe_dhcp::wire::{Message, Op, code};
use xpxe_test_support::{TestResult, ensure_eq};

use super::{Config, Decision, decide};

/// 允许的 MAC。
const MAC: [u8; 6] = [0x52, 0x54, 0, 0x77, 1, 1];

/// 测试配置。
fn config() -> Config {
    let mut efi_override = BTreeMap::new();
    let _previous = efi_override.insert([0x52, 0x54, 0, 0x77, 1, 2], "snponly.efi".to_owned());
    Config {
        server: Ipv4Addr::new(10, 77, 0, 1),
        allow: vec![MAC, [0x52, 0x54, 0, 0x77, 1, 2]],
        boot_base: "http://10.77.0.1:8080".to_owned(),
        efi_file: "ipxe.efi".to_owned(),
        efi_override,
        bios_file: "undionly.kpxe".to_owned(),
    }
}

/// 构造请求。
fn request(mac: [u8; 6], message_type: u8, options: Vec<(u8, Vec<u8>)>) -> Message {
    let mut chaddr = [0_u8; 16];
    chaddr
        .iter_mut()
        .zip(mac)
        .for_each(|(slot, byte)| *slot = byte);
    let mut all = vec![(code::MESSAGE_TYPE, vec![message_type])];
    all.extend(options);
    Message {
        op: Op::Request,
        htype: 1,
        hlen: 6,
        hops: 0,
        xid: 1,
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

/// PXE 厂商类别与架构。
fn pxe(arch: u8) -> Vec<(u8, Vec<u8>)> {
    vec![
        (
            code::VENDOR_CLASS,
            b"PXEClient:Arch:00007:UNDI:003016".to_vec(),
        ),
        (code::CLIENT_ARCH, vec![0, arch]),
    ]
}

/// 各类客户端的决策。
///
/// # Errors
///
/// 决策不符时返回错误。
#[test]
fn decisions() -> TestResult {
    let cfg = config();
    let tftp = |file: &str| BootTarget::Tftp {
        server: Ipv4Addr::new(10, 77, 0, 1),
        file: file.to_owned(),
    };
    ensure_eq(
        &decide(&cfg, &request(MAC, 1, pxe(7)), 67),
        &Decision::Reply(ReplyKind::Offer, tftp("ipxe.efi")),
        "UEFI",
    )?;
    ensure_eq(
        &decide(&cfg, &request(MAC, 3, pxe(7)), 4011),
        &Decision::Reply(ReplyKind::Ack, tftp("ipxe.efi")),
        "4011",
    )?;
    ensure_eq(
        &decide(&cfg, &request(MAC, 1, pxe(0)), 67),
        &Decision::Reply(ReplyKind::Offer, tftp("undionly.kpxe")),
        "BIOS",
    )?;
    ensure_eq(
        &decide(&cfg, &request([0x52, 0x54, 0, 0x77, 1, 2], 1, pxe(7)), 67),
        &Decision::Reply(ReplyKind::Offer, tftp("snponly.efi")),
        "按 MAC 覆盖",
    )?;
    let http = vec![
        (
            code::VENDOR_CLASS,
            b"HTTPClient:Arch:00016:UNDI:003001".to_vec(),
        ),
        (code::CLIENT_ARCH, vec![0, 16]),
    ];
    ensure_eq(
        &decide(&cfg, &request(MAC, 1, http), 67),
        &Decision::Reply(
            ReplyKind::Offer,
            BootTarget::HttpBoot {
                url: "http://10.77.0.1:8080/fw/x64/ipxe.efi".to_owned(),
            },
        ),
        "HTTP Boot",
    )?;
    let ipxe = vec![
        (code::IPXE_ENCAP, vec![1, 1, 1]),
        (code::CLIENT_ARCH, vec![0, 7]),
    ];
    ensure_eq(
        &decide(&cfg, &request(MAC, 1, ipxe), 67),
        &Decision::Reply(
            ReplyKind::Offer,
            BootTarget::IpxeScript {
                url: "http://10.77.0.1:8080/boot.ipxe".to_owned(),
            },
        ),
        "仅 Opt175 也判为 iPXE",
    )?;
    Ok(())
}

/// 不应答的情形。
///
/// # Errors
///
/// 决策不符时返回错误。
#[test]
fn ignores() -> TestResult {
    let cfg = config();
    ensure_eq(
        &decide(&cfg, &request([9; 6], 1, pxe(7)), 67),
        &Decision::Ignore("mac-not-allowed"),
        "MAC",
    )?;
    ensure_eq(
        &decide(&cfg, &request(MAC, 1, Vec::new()), 67),
        &Decision::Ignore("not-pxe"),
        "普通 DHCP",
    )?;
    ensure_eq(
        &decide(&cfg, &request(MAC, 3, pxe(7)), 67),
        &Decision::Ignore("message-type"),
        "67 上发给别人的 REQUEST",
    )?;
    let mut to_us = pxe(7);
    to_us.push((code::SERVER_ID, vec![10, 77, 0, 1]));
    ensure_eq(
        &decide(&cfg, &request(MAC, 3, to_us), 67),
        &Decision::Reply(
            ReplyKind::Ack,
            BootTarget::Tftp {
                server: Ipv4Addr::new(10, 77, 0, 1),
                file: "ipxe.efi".to_owned(),
            },
        ),
        "67 上发给本机的 REQUEST",
    )?;
    Ok(())
}
