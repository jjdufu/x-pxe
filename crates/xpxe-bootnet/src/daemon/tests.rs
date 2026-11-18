//! `daemon` 模块单元测试：发布器的失联关闭、恢复与网卡白名单。

use std::sync::Arc;
use std::time::{Duration, Instant};

use xpxe_domain::{
    BootNetwork, BootToken, DhcpRules, DirectiveSnapshot, FirmwareSet, Health, HttpUrl, MacAddr,
    NetIface, SessionId, Sha256Hex, SnapshotEntry, Timestamp,
};
use xpxe_test_support::{TestResult, ensure, ensure_eq};

use super::Publisher;
use crate::state::Shared;

/// 一条指令的快照。
///
/// # Errors
///
/// 值解析失败。
fn snapshot(iface: &str) -> TestResult<DirectiveSnapshot> {
    let set = FirmwareSet {
        undionly: Sha256Hex::parse(&"1".repeat(64))?,
        ipxe_efi: Sha256Hex::parse(&"2".repeat(64))?,
        snponly: Sha256Hex::parse(&"3".repeat(64))?,
    };
    Ok(DirectiveSnapshot {
        rev: 5,
        network: BootNetwork::ProxyDhcp {
            iface: NetIface::parse(iface)?,
        },
        rules: DhcpRules::standard(),
        boot_base_url: HttpUrl::parse("http://10.77.0.1:8080")?,
        active_firmware: None,
        directives: vec![SnapshotEntry {
            mac: MacAddr::parse("52:54:00:77:01:b1")?,
            session: SessionId::parse("s")?,
            boot_token: BootToken::parse("token-0000")?,
            firmware: set,
            uefi_firmware: None,
            expires_at: Timestamp::from_unix_seconds(u64::MAX),
        }],
        acknowledged_conflicts: Vec::new(),
    })
}

/// 失联 30 s 后指令表为空且降级；恢复后重新发布。
///
/// # Errors
///
/// 断言失败。
#[test]
fn failsafe_and_recovery() -> TestResult {
    let start = Instant::now();
    let shared = Arc::new(Shared::default());
    let (publisher, receiver) =
        Publisher::new(vec![NetIface::parse("br-lab")?], Arc::clone(&shared), start)?;
    ensure_eq(
        &receiver.borrow().network,
        &BootNetwork::RedfishOnly,
        "启动时关闭",
    )?;
    publisher.accept(snapshot("br-lab")?, start);
    ensure_eq(&receiver.borrow().directives.len(), &1, "应用快照")?;
    ensure_eq(&publisher.applied_rev(), &Some(5), "版本")?;
    let at = |secs| start.checked_add(Duration::from_secs(secs)).ok_or("时钟");
    publisher.tick(at(30)?);
    ensure_eq(&receiver.borrow().directives.len(), &1, "30 s 内仍应答")?;
    publisher.tick(at(31)?);
    ensure(receiver.borrow().directives.is_empty(), "失联后清空")?;
    ensure_eq(&shared.health().0, &Health::Degraded, "降级")?;
    publisher.keepalive(at(32)?);
    ensure_eq(&receiver.borrow().directives.len(), &1, "恢复后重新发布")?;
    ensure_eq(&shared.health().0, &Health::Ok, "恢复 ok")?;
    Ok(())
}

/// 白名单外的网卡：降级并按 `RedfishOnly` 关闭监听。
///
/// # Errors
///
/// 断言失败。
#[test]
fn rejects_unlisted_iface() -> TestResult {
    let start = Instant::now();
    let shared = Arc::new(Shared::default());
    let (publisher, receiver) =
        Publisher::new(vec![NetIface::parse("br-lab")?], Arc::clone(&shared), start)?;
    publisher.accept(snapshot("eth9")?, start);
    ensure_eq(
        &receiver.borrow().network,
        &BootNetwork::RedfishOnly,
        "关闭监听",
    )?;
    ensure(receiver.borrow().directives.is_empty(), "不应答")?;
    ensure_eq(&shared.health().0, &Health::Degraded, "降级")?;
    Ok(())
}
