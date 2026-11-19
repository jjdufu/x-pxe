//! `memory` 模块单元测试：TFTP 路径解析与缓存。

use std::collections::BTreeSet;
use std::sync::Arc;

use tokio::sync::watch;
use xpxe_domain::{
    BootNetwork, BootToken, DhcpRules, DirectiveSnapshot, FirmwareSet, HttpUrl, MacAddr, NetIface,
    SessionId, Sha256Hex, SnapshotEntry, Timestamp,
};
use xpxe_test_support::{TestResult, ensure, ensure_eq};
use xpxe_tftp::server::Source as _;

use super::{FirmwareCache, TftpFiles, resolve};

/// 由单字符构造摘要。
///
/// # Errors
///
/// 解析失败。
fn digest(ch: char) -> TestResult<Sha256Hex> {
    Ok(Sha256Hex::parse(&ch.to_string().repeat(64))?)
}

/// 路径解析、别名与缓存命中。
///
/// # Errors
///
/// 断言失败。
#[test]
fn resolves_paths() -> TestResult {
    let active = FirmwareSet {
        undionly: digest('1')?,
        ipxe_efi: digest('2')?,
        snponly: digest('3')?,
    };
    let canary = FirmwareSet {
        undionly: digest('4')?,
        ipxe_efi: digest('5')?,
        snponly: digest('6')?,
    };
    let snap = DirectiveSnapshot {
        rev: 1,
        network: BootNetwork::ExternalDhcp {
            iface: NetIface::parse("br-lab")?,
        },
        rules: DhcpRules::standard(),
        boot_base_url: HttpUrl::parse("http://10.77.0.1:8080")?,
        active_firmware: Some(active),
        directives: vec![SnapshotEntry {
            mac: MacAddr::parse("52:54:00:77:01:b1")?,
            session: SessionId::parse("s")?,
            boot_token: BootToken::parse("token-0000")?,
            firmware: canary,
            uefi_firmware: None,
            expires_at: Timestamp::from_unix_seconds(9),
        }],
        acknowledged_conflicts: Vec::new(),
        probe_nonce: 0,
    };
    ensure_eq(
        &resolve(&snap, "undionly.kpxe"),
        &Some(digest('1')?),
        "别名指向激活版本",
    )?;
    ensure_eq(
        &resolve(&snap, "fw/555555555555/ipxe.efi"),
        &Some(digest('5')?),
        "节点固件",
    )?;
    ensure_eq(
        &resolve(&snap, "fw/222222222222/ipxe.efi"),
        &Some(digest('2')?),
        "激活固件路径",
    )?;
    ensure_eq(
        &resolve(&snap, "fw/555555555555/undionly.kpxe"),
        &None,
        "名称与摘要不符",
    )?;
    ensure_eq(&resolve(&snap, "etc/passwd"), &None, "未知路径")?;
    caches(snap)
}

/// 缓存：缺失、清理与 TFTP 查找。
///
/// # Errors
///
/// 断言失败。
fn caches(snap: DirectiveSnapshot) -> TestResult {
    let cache = Arc::new(FirmwareCache::default());
    let wanted = snap.referenced_firmware();
    ensure_eq(&cache.missing(&wanted).len(), &6, "全部缺失")?;
    cache.insert(digest('1')?, Arc::from(vec![9_u8; 3]));
    cache.insert(digest('9')?, Arc::from(vec![0_u8; 1]));
    cache.retain(&wanted);
    ensure(cache.get(&digest('9')?).is_none(), "未引用的被清理")?;
    let (_sender, receiver) = watch::channel(Arc::new(snap));
    let files = TftpFiles::new(Arc::clone(&cache), receiver);
    ensure_eq(
        &files.open("undionly.kpxe").map(|data| data.len()),
        &Some(3),
        "TFTP 取到缓存内容",
    )?;
    ensure(files.open("ipxe.efi").is_none(), "未缓存的不提供")?;
    ensure(cache.missing(&BTreeSet::new()).is_empty(), "空集合")?;
    Ok(())
}
