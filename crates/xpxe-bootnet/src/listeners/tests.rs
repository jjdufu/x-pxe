//! `listeners` 模块单元测试：内存传输上的收发循环，回环网卡上的模式切换。

use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::sync::watch;
use xpxe_dhcp::link::MemoryTransport;
use xpxe_dhcp::wire::{Message, Op, code};
use xpxe_domain::{
    BootNetwork, BootToken, DhcpRules, DirectiveSnapshot, FirmwareSet, HttpUrl, MacAddr, NetIface,
    SessionId, Sha256Hex, SnapshotEntry, Timestamp,
};
use xpxe_test_support::{TestFailure, TestResult, ensure, ensure_eq};

use super::{DhcpLoop, Ports, Supervisor};
use crate::answering::Engine;
use crate::memory::FirmwareCache;
use crate::state::Shared;

/// 构造快照。
///
/// # Errors
///
/// 值解析失败。
fn snapshot(network: BootNetwork, expires: u64) -> TestResult<DirectiveSnapshot> {
    let set = FirmwareSet {
        undionly: Sha256Hex::parse(&"1".repeat(64))?,
        ipxe_efi: Sha256Hex::parse(&"2".repeat(64))?,
        snponly: Sha256Hex::parse(&"3".repeat(64))?,
    };
    Ok(DirectiveSnapshot {
        rev: 1,
        network,
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

/// iPXE 的 DISCOVER。
fn ipxe_discover(last: u8) -> Message {
    let mut chaddr = [0_u8; 16];
    for (slot, byte) in chaddr.iter_mut().zip([0x52, 0x54, 0, 0x77, 1, last]) {
        *slot = byte;
    }
    Message {
        op: Op::Request,
        htype: 1,
        hlen: 6,
        hops: 0,
        xid: 5,
        secs: 0,
        flags: 0,
        ciaddr: Ipv4Addr::UNSPECIFIED,
        yiaddr: Ipv4Addr::UNSPECIFIED,
        siaddr: Ipv4Addr::UNSPECIFIED,
        giaddr: Ipv4Addr::UNSPECIFIED,
        chaddr,
        sname: Vec::new(),
        file: Vec::new(),
        options: vec![
            (code::MESSAGE_TYPE, vec![1]),
            (
                code::VENDOR_CLASS,
                b"PXEClient:Arch:00000:UNDI:002001".to_vec(),
            ),
            (code::USER_CLASS, b"iPXE".to_vec()),
            (code::CLIENT_ARCH, vec![0, 0]),
        ],
    }
}

/// 判定集成：未武装不应答；武装后应答；指令过期后不应答；失联（指令清空）后不应答。
///
/// # Errors
///
/// 断言失败。
#[test]
fn dhcp_loop_follows_snapshot() -> TestResult {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let network = BootNetwork::ProxyDhcp {
            iface: NetIface::parse("lo")?,
        };
        let far = u64::MAX;
        let (sender, receiver) = watch::channel(Arc::new(snapshot(network.clone(), far)?));
        let shared = Arc::new(Shared::default());
        let job = DhcpLoop {
            engine: Arc::new(Mutex::new(Engine::new(Instant::now()))),
            shared: Arc::clone(&shared),
            snapshot: receiver,
            server: Ipv4Addr::new(10, 77, 0, 1),
        };
        let (transport, mut peer) = MemoryTransport::pair(67);
        let task = tokio::spawn(job.run(transport));
        let from = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 68);
        let wait = Duration::from_millis(300);

        peer.inject(ipxe_discover(0xf1).encode()?, from)?;
        ensure(peer.next_reply(wait).await.is_none(), "未武装不应答")?;

        peer.inject(ipxe_discover(0xb1).encode()?, from)?;
        let (bytes, _to) = peer
            .next_reply(wait)
            .await
            .ok_or_else(|| TestFailure::new("武装应应答"))?;
        let reply = Message::decode(&bytes)?;
        ensure_eq(
            &reply.file.as_slice(),
            &b"http://10.77.0.1:8080/p/token-b1-0000/boot.ipxe".as_slice(),
            "iPXE 脚本 URL",
        )?;
        ensure_eq(&reply.siaddr, &Ipv4Addr::new(10, 77, 0, 1), "siaddr 非零")?;

        let _old = sender.send_replace(Arc::new(snapshot(network.clone(), 1)?));
        peer.inject(ipxe_discover(0xb1).encode()?, from)?;
        ensure(peer.next_reply(wait).await.is_none(), "过期不应答")?;

        let stale = crate::failsafe::effective(&snapshot(network, far)?, true);
        let _old2 = sender.send_replace(Arc::new(stale));
        peer.inject(ipxe_discover(0xb1).encode()?, from)?;
        ensure(peer.next_reply(wait).await.is_none(), "失联清空后不应答")?;

        let counters = shared.counters();
        ensure_eq(
            &(counters.dhcp_received, counters.dhcp_replies),
            &(4, 1),
            "计数",
        )?;
        task.abort();
        Ok(())
    })
}

/// 端口是否空闲（不带 `SO_REUSEADDR` 绑定成功即空闲）。
fn port_free(port: u16) -> bool {
    UdpSocket::bind((Ipv4Addr::UNSPECIFIED, port)).is_ok()
}

/// 等待条件成立（最多 3 s）。
async fn eventually<F: Fn() -> bool + Send>(check: F) -> bool {
    for _ in 0_u8..30 {
        if check() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    false
}

/// 模式切换：`ProxyDhcp` 打开三个端口，`ExternalDhcp` 只留 TFTP，`RedfishOnly` 全部关闭。
///
/// # Errors
///
/// 断言失败。
#[test]
fn mode_switch_closes_ports() -> TestResult {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let base = 20_000_u16.saturating_add(u16::try_from(std::process::id() % 20_000)?);
        let ports = Ports {
            dhcp: base,
            pxe: base.saturating_add(1),
            tftp: base.saturating_add(2),
        };
        let lo = NetIface::parse("lo")?;
        let (sender, receiver) = watch::channel(Arc::new(snapshot(
            BootNetwork::ProxyDhcp { iface: lo.clone() },
            9,
        )?));
        let shared = Arc::new(Shared::default());
        let supervisor = Supervisor::new(
            Arc::clone(&shared),
            Arc::new(FirmwareCache::default()),
            ports,
        );
        let task = tokio::spawn(supervisor.run(receiver));
        let all = vec![ports.dhcp, ports.pxe, ports.tftp];
        ensure(
            eventually(|| shared.listening() == all).await,
            "ProxyDhcp 监听三个端口",
        )?;
        ensure(
            !port_free(ports.dhcp) && !port_free(ports.tftp),
            "端口被占用",
        )?;

        let _old = sender.send_replace(Arc::new(snapshot(
            BootNetwork::ExternalDhcp { iface: lo },
            9,
        )?));
        ensure(
            eventually(|| shared.listening() == vec![ports.tftp]).await,
            "ExternalDhcp 只留 TFTP",
        )?;
        ensure(
            eventually(|| port_free(ports.dhcp) && port_free(ports.pxe)).await,
            "67/4011 已释放",
        )?;

        let _old2 = sender.send_replace(Arc::new(snapshot(BootNetwork::RedfishOnly, 9)?));
        ensure(
            eventually(|| shared.listening().is_empty()).await,
            "RedfishOnly 无监听",
        )?;
        ensure(
            eventually(|| all.iter().all(|port| port_free(*port))).await,
            "全部端口已释放",
        )?;
        drop(sender);
        task.await?;
        Ok(())
    })
}
