//! `answering` 模块单元测试：限速、准入、各类请求的处理结论。

use std::net::{Ipv4Addr, SocketAddrV4};
use std::time::{Duration, Instant};

use xpxe_dhcp::wire::{Message, Op, code};
use xpxe_domain::{
    BootNetwork, BootToken, DhcpRules, DirectiveSnapshot, FirmwareSet, HttpUrl, MacAddr, NetIface,
    Observation, SessionId, Sha256Hex, SnapshotEntry, Timestamp,
};
use xpxe_test_support::{TestFailure, TestResult, ensure, ensure_eq};

use super::{Context, Engine, Inbound, RateLimiter, Verdict};
use crate::state::Counter;

/// 已武装的 MAC。
const ARMED: [u8; 6] = [0x52, 0x54, 0, 0x77, 1, 0xb1];

/// 构造快照（`ARMED` 在 `expires` 前有效）。
///
/// # Errors
///
/// 值解析失败。
fn snapshot(expires: u64) -> TestResult<DirectiveSnapshot> {
    let set = FirmwareSet {
        undionly: Sha256Hex::parse(&"1".repeat(64))?,
        ipxe_efi: Sha256Hex::parse(&"2".repeat(64))?,
        snponly: Sha256Hex::parse(&"3".repeat(64))?,
    };
    Ok(DirectiveSnapshot {
        rev: 1,
        network: BootNetwork::ProxyDhcp {
            iface: NetIface::parse("br-lab")?,
        },
        rules: DhcpRules::standard(),
        boot_base_url: HttpUrl::parse("http://10.77.0.1:8080")?,
        active_firmware: Some(set.clone()),
        directives: vec![SnapshotEntry {
            mac: MacAddr::parse("52:54:00:77:01:b1")?,
            session: SessionId::parse("s-b1")?,
            boot_token: BootToken::parse("token-b1-0000")?,
            firmware: set,
            uefi_firmware: None,
            expires_at: Timestamp::from_unix_seconds(expires),
        }],
        acknowledged_conflicts: Vec::new(),
    })
}

/// 构造请求报文。
fn request(mac: [u8; 6], message_type: u8, extra: Vec<(u8, Vec<u8>)>) -> Message {
    let mut chaddr = [0_u8; 16];
    for (slot, byte) in chaddr.iter_mut().zip(mac) {
        *slot = byte;
    }
    let mut options = vec![
        (code::MESSAGE_TYPE, vec![message_type]),
        (
            code::VENDOR_CLASS,
            b"PXEClient:Arch:00007:UNDI:003016".to_vec(),
        ),
        (code::CLIENT_ARCH, vec![0, 7]),
    ];
    options.extend(extra);
    Message {
        op: Op::Request,
        htype: 1,
        hlen: 6,
        hops: 0,
        xid: 42,
        secs: 0,
        flags: 0,
        ciaddr: Ipv4Addr::UNSPECIFIED,
        yiaddr: Ipv4Addr::UNSPECIFIED,
        siaddr: Ipv4Addr::UNSPECIFIED,
        giaddr: Ipv4Addr::UNSPECIFIED,
        chaddr,
        sname: Vec::new(),
        file: Vec::new(),
        options,
    }
}

/// 处理一个报文。
///
/// # Errors
///
/// 编码失败。
fn run(
    engine: &mut Engine,
    message: &Message,
    port: u16,
    snap: &DirectiveSnapshot,
    paused: bool,
) -> TestResult<Verdict> {
    let bytes = message.encode()?;
    let ctx = Context {
        snapshot: snap,
        server: Ipv4Addr::new(10, 77, 0, 1),
        paused,
        now: Instant::now(),
        wall: Timestamp::from_unix_seconds(1_000),
    };
    let from = SocketAddrV4::new(
        Ipv4Addr::new(10, 77, 0, 50),
        if port == 67 { 68 } else { 4011 },
    );
    Ok(engine.handle(
        Inbound {
            payload: &bytes,
            port,
            from,
        },
        &ctx,
    ))
}

/// 未武装、武装、过期、暂停、4011 ACK。
///
/// # Errors
///
/// 断言失败。
#[test]
fn armed_and_unarmed() -> TestResult {
    let snap = snapshot(2_000)?;
    let mut engine = Engine::new(Instant::now());
    let other = request([0x52, 0x54, 0, 0x77, 1, 0xf1], 1, Vec::new());
    match run(&mut engine, &other, 67, &snap, false)? {
        Verdict::Ignored(observation) => ensure(
            matches!(&*observation, Observation::DhcpIgnore { reason, .. } if reason == "not_armed"),
            "未武装写 not_armed 观测",
        )?,
        other_verdict @ (Verdict::Dropped(_) | Verdict::Unrelated | Verdict::Replied { .. }) => {
            return Err(Box::new(TestFailure::new(format!(
                "未武装应忽略：{other_verdict:?}"
            ))));
        }
    }
    let armed = request(ARMED, 1, vec![(code::CLIENT_UUID, vec![0, 1])]);
    let Verdict::Replied { bytes, to, .. } = run(&mut engine, &armed, 67, &snap, false)? else {
        return Err(Box::new(TestFailure::new("武装 MAC 应应答")));
    };
    let reply = Message::decode(&bytes)?;
    ensure_eq(
        &reply.file.as_slice(),
        &b"fw/222222222222/ipxe.efi".as_slice(),
        "UEFI 固件路径",
    )?;
    ensure_eq(&to, &SocketAddrV4::new(Ipv4Addr::BROADCAST, 68), "广播")?;
    let expired = snapshot(500)?;
    ensure(
        matches!(
            run(&mut engine, &armed, 67, &expired, false)?,
            Verdict::Ignored(_)
        ),
        "过期不应答",
    )?;
    ensure_eq(
        &run(&mut engine, &armed, 67, &snap, true)?,
        &Verdict::Dropped(Counter::DhcpPaused),
        "冲突暂停",
    )?;
    let ack = request(ARMED, 3, Vec::new());
    let Verdict::Replied { to: ack_to, .. } = run(&mut engine, &ack, 4011, &snap, false)? else {
        return Err(Box::new(TestFailure::new("4011 REQUEST 应 ACK")));
    };
    ensure_eq(&ack_to.port(), &4011, "单播回源")?;
    Ok(())
}

/// 无关报文、畸形与超长报文。
///
/// # Errors
///
/// 断言失败。
#[test]
fn unrelated_and_malformed() -> TestResult {
    let snap = snapshot(2_000)?;
    let mut engine = Engine::new(Instant::now());
    ensure_eq(
        &run(
            &mut engine,
            &request(ARMED, 3, Vec::new()),
            67,
            &snap,
            false,
        )?,
        &Verdict::Unrelated,
        "67 REQUEST",
    )?;
    let mut plain = request([1, 2, 3, 4, 5, 6], 1, Vec::new());
    plain
        .options
        .retain(|(option, _)| *option != code::VENDOR_CLASS);
    ensure_eq(
        &run(&mut engine, &plain, 67, &snap, false)?,
        &Verdict::Unrelated,
        "非 PXE 客户端",
    )?;
    let probe = request([0x02, 0x78, 0x70, 0x78, 0x65, 9], 1, Vec::new());
    ensure_eq(
        &run(&mut engine, &probe, 67, &snap, false)?,
        &Verdict::Unrelated,
        "自身探测",
    )?;
    Ok(())
}

/// 畸形与超长报文。
///
/// # Errors
///
/// 断言失败。
#[test]
fn malformed_packets() -> TestResult {
    let snap = snapshot(2_000)?;
    let mut engine = Engine::new(Instant::now());
    let ctx = Context {
        snapshot: &snap,
        server: Ipv4Addr::new(10, 77, 0, 1),
        paused: false,
        now: Instant::now(),
        wall: Timestamp::from_unix_seconds(1_000),
    };
    let from = SocketAddrV4::new(Ipv4Addr::new(10, 77, 0, 50), 68);
    let huge = vec![0_u8; 1_501];
    ensure_eq(
        &engine.handle(
            Inbound {
                payload: &huge,
                port: 67,
                from,
            },
            &ctx,
        ),
        &Verdict::Dropped(Counter::DhcpMalformed),
        "超长",
    )?;
    ensure_eq(
        &engine.handle(
            Inbound {
                payload: &[1, 2, 3],
                port: 67,
                from,
            },
            &ctx,
        ),
        &Verdict::Dropped(Counter::DhcpMalformed),
        "截断",
    )?;
    Ok(())
}

/// 限速：全局每秒 200、每 MAC 每分钟 20。
///
/// # Errors
///
/// 断言失败。
#[test]
fn rate_limits() -> TestResult {
    let start = Instant::now();
    let mut limiter = RateLimiter::new(start);
    let admitted = (0_u32..250).filter(|_| limiter.admit_packet(start)).count();
    ensure_eq(&admitted, &200, "全局每秒 200")?;
    let later = start.checked_add(Duration::from_secs(1)).ok_or("时钟")?;
    ensure(limiter.admit_packet(later), "下一秒恢复")?;
    let mac = MacAddr::parse("52:54:00:77:01:b1")?;
    let replies = (0_u32..25)
        .filter(|_| limiter.admit_reply(mac, start))
        .count();
    ensure_eq(&replies, &20, "每 MAC 每分钟 20")?;
    let other = MacAddr::parse("52:54:00:77:01:b2")?;
    ensure(limiter.admit_reply(other, start), "其他 MAC 不受影响")?;
    let minute = start.checked_add(Duration::from_secs(60)).ok_or("时钟")?;
    ensure(limiter.admit_reply(mac, minute), "一分钟后恢复")?;
    Ok(())
}
