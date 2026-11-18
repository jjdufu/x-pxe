//! `scan` 模块单元测试：内存传输模拟三类应答者。

use std::net::{Ipv4Addr, SocketAddrV4};
use std::time::Duration;

use xpxe_dhcp::link::MemoryTransport;
use xpxe_dhcp::wire::{Message, MessageType, Op, code};
use xpxe_domain::ResponderClass;
use xpxe_test_support::{TestFailure, TestResult, ensure, ensure_eq};

use super::{probe, probe_identity};
use crate::answering::PROBE_MAC_PREFIX;

/// 由探测报文构造一个 OFFER。
fn offer(
    request: &Message,
    server: [u8; 4],
    yiaddr: [u8; 4],
    extra: Vec<(u8, Vec<u8>)>,
) -> Message {
    let mut reply = request.clone();
    reply.op = Op::Reply;
    reply.yiaddr = Ipv4Addr::from(yiaddr);
    reply.options = vec![
        (code::MESSAGE_TYPE, vec![MessageType::Offer.to_byte()]),
        (code::SERVER_ID, server.to_vec()),
    ];
    reply.options.extend(extra);
    reply
}

/// 探测 MAC 前缀。
///
/// # Errors
///
/// 断言失败。
#[test]
fn identity_prefix() -> TestResult {
    let (mac, _xid) = probe_identity(0x0102_0304_0506_0708);
    ensure(mac.starts_with(&PROBE_MAC_PREFIX), "固定前缀")?;
    ensure_eq(&mac.last(), &Some(&0x04), "末字节取自种子")?;
    Ok(())
}

/// 三类应答者都被收集并分类；重复服务器去重。
///
/// # Errors
///
/// 断言失败。
#[test]
fn collects_three_kinds() -> TestResult {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let (transport, mut peer) = MemoryTransport::pair(68);
        let seed = 77;
        let task =
            tokio::spawn(async move { probe(&transport, seed, Duration::from_millis(300)).await });
        let (sent, to) = peer
            .next_reply(Duration::from_secs(1))
            .await
            .ok_or_else(|| TestFailure::new("未发出探测"))?;
        ensure_eq(
            &to,
            &SocketAddrV4::new(Ipv4Addr::BROADCAST, 67),
            "广播到 67",
        )?;
        let discover = Message::decode(&sent)?;
        let from = SocketAddrV4::new(Ipv4Addr::new(10, 77, 0, 2), 67);
        let normal = offer(&discover, [10, 77, 0, 2], [10, 77, 0, 120], Vec::new());
        let mut rogue = offer(
            &discover,
            [10, 77, 0, 3],
            [10, 77, 0, 121],
            vec![(code::BOOTFILE, b"x.efi".to_vec())],
        );
        rogue.siaddr = Ipv4Addr::new(10, 77, 0, 3);
        let proxy = offer(
            &discover,
            [10, 77, 0, 4],
            [0, 0, 0, 0],
            vec![(code::VENDOR_CLASS, b"PXEClient".to_vec())],
        );
        let mut stale = normal.clone();
        stale.xid = stale.xid.wrapping_add(1);
        for message in [&normal, &rogue, &proxy, &normal, &stale] {
            peer.inject(message.encode()?, from)?;
        }
        let found = task.await??;
        let classes: Vec<(Ipv4Addr, ResponderClass)> =
            found.iter().map(|item| (item.server, item.class)).collect();
        ensure_eq(
            &classes,
            &vec![
                (Ipv4Addr::new(10, 77, 0, 2), ResponderClass::AddressOnly),
                (Ipv4Addr::new(10, 77, 0, 3), ResponderClass::PxeCapable),
                (Ipv4Addr::new(10, 77, 0, 4), ResponderClass::Proxy),
            ],
            "分类与去重",
        )?;
        Ok(())
    })
}
