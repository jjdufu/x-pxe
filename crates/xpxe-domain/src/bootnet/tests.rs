//! 引导网络载荷：校验、快照转换、探测结论与健康判定。

use std::net::Ipv4Addr;

use xpxe_test_support::{TestResult, ensure, ensure_eq};

use super::{
    BootNetwork, DirectiveSnapshot, ExternalCheck, FirmwareSet, Health, HeartbeatRecord, NetIface,
    ProbeReport, Responder, ResponderClass, Sha256Hex, SnapshotEntry,
};
use crate::decision::{DhcpRules, FirmwareName, UefiFirmware};
use crate::lifecycle::{SessionId, Timestamp};
use crate::value::{BootToken, HttpUrl, MacAddr};

/// 构造一个固件集。
///
/// # Errors
///
/// 摘要解析失败。
fn firmware() -> TestResult<FirmwareSet> {
    Ok(FirmwareSet {
        undionly: Sha256Hex::parse(&"a".repeat(64))?,
        ipxe_efi: Sha256Hex::parse(&"B".repeat(64))?,
        snponly: Sha256Hex::parse(&"c".repeat(64))?,
    })
}

/// 构造一个只有一条指令的快照。
///
/// # Errors
///
/// 值解析失败。
fn snapshot() -> TestResult<DirectiveSnapshot> {
    Ok(DirectiveSnapshot {
        rev: 3,
        network: BootNetwork::ProxyDhcp {
            iface: NetIface::parse("br-lab")?,
        },
        rules: DhcpRules::standard(),
        boot_base_url: HttpUrl::parse("http://10.77.0.1:8080")?,
        active_firmware: Some(firmware()?),
        directives: vec![SnapshotEntry {
            mac: MacAddr::parse("52:54:00:77:01:c6")?,
            session: SessionId::parse("s1")?,
            boot_token: BootToken::parse("tok-c6-000000")?,
            firmware: firmware()?,
            uefi_firmware: Some(UefiFirmware::SnpOnlyEfi),
            expires_at: Timestamp::from_unix_seconds(9_999),
        }],
        acknowledged_conflicts: Vec::new(),
        probe_nonce: 0,
    })
}

/// 网卡名与摘要校验。
///
/// # Errors
///
/// 断言失败。
#[test]
fn value_parsing() -> TestResult {
    ensure(NetIface::parse("ens192").is_ok(), "ens192 合法")?;
    ensure(NetIface::parse("").is_err(), "空名非法")?;
    ensure(
        NetIface::parse("a234567890123456").is_err(),
        "超过 15 字节非法",
    )?;
    ensure(NetIface::parse("br lab").is_err(), "空格非法")?;
    ensure(NetIface::parse("..").is_err(), "`..` 非法")?;
    ensure(Sha256Hex::parse("abc").is_err(), "长度错误")?;
    ensure(Sha256Hex::parse(&"g".repeat(64)).is_err(), "非十六进制")?;
    let digest = Sha256Hex::parse(&"AB".repeat(32))?;
    ensure_eq(&digest.as_str(), &"ab".repeat(32).as_str(), "规范为小写")?;
    ensure_eq(&digest.short12(), &"abababababab", "前 12 位")?;
    Ok(())
}

/// 模式 JSON 形状与白名单检查。
///
/// # Errors
///
/// 断言失败。
#[test]
fn network_modes() -> TestResult {
    let proxy: BootNetwork = serde_json::from_str(r#"{"mode":"proxy_dhcp","iface":"br-lab"}"#)?;
    ensure(proxy.serves_dhcp(), "proxy_dhcp 监听 67")?;
    let only: BootNetwork = serde_json::from_str(r#"{"mode":"redfish_only"}"#)?;
    ensure_eq(&only.iface(), &None, "redfish_only 无网卡")?;
    ensure_eq(&only.label(), &"redfish_only", "模式名")?;
    let allowed = vec![NetIface::parse("ens192")?];
    ensure(proxy.check_allowed(&allowed).is_err(), "白名单外拒绝")?;
    ensure(
        only.check_allowed(&allowed).is_ok(),
        "redfish_only 不需网卡",
    )?;
    ensure(
        serde_json::from_str::<BootNetwork>(r#"{"mode":"proxy_dhcp","iface":"x y"}"#).is_err(),
        "非法网卡名拒绝反序列化",
    )?;
    Ok(())
}

/// 快照转换为决策指令表、固件路径与按节点覆盖。
///
/// # Errors
///
/// 断言失败。
#[test]
fn snapshot_helpers() -> TestResult {
    let snap = snapshot()?;
    let json = serde_json::to_string(&snap)?;
    let back: DirectiveSnapshot = serde_json::from_str(&json)?;
    ensure_eq(&back, &snap, "JSON 往返")?;
    let mac = MacAddr::parse("52:54:00:77:01:c6")?;
    ensure(snap.table().find(mac).is_some(), "指令表含该 MAC")?;
    ensure_eq(
        &snap.rules_for(mac).uefi_firmware(),
        &UefiFirmware::SnpOnlyEfi,
        "按节点覆盖 UEFI 固件",
    )?;
    let other = MacAddr::parse("52:54:00:77:01:c1")?;
    ensure_eq(
        &snap.rules_for(other).uefi_firmware(),
        &UefiFirmware::IpxeEfi,
        "其他节点用全局规则",
    )?;
    ensure_eq(&snap.referenced_firmware().len(), &3, "三个摘要去重")?;
    let set = firmware()?;
    ensure_eq(
        &set.tftp_path(FirmwareName::IpxeEfi).as_str(),
        &"fw/bbbbbbbbbbbb/ipxe.efi",
        "TFTP 路径",
    )?;
    let closed = DirectiveSnapshot::closed(0, BootNetwork::RedfishOnly, snap.boot_base_url);
    ensure(closed.directives.is_empty(), "空快照无指令")?;
    Ok(())
}

/// 构造应答者。
fn responder(
    server: [u8; 4],
    class: ResponderClass,
    siaddr: [u8; 4],
    file: Option<&str>,
) -> Responder {
    Responder {
        server: Ipv4Addr::from(server),
        class,
        siaddr: Ipv4Addr::from(siaddr),
        file: file.map(str::to_owned),
        options: Vec::new(),
    }
}

/// 探测结论：冲突、确认、复用现网 DHCP 的三种结论。
///
/// # Errors
///
/// 断言失败。
#[test]
fn probe_assessment() -> TestResult {
    let at = Timestamp::from_unix_seconds(1);
    let local = Ipv4Addr::new(10, 77, 0, 1);
    let proxy = BootNetwork::ProxyDhcp {
        iface: NetIface::parse("br-lab")?,
    };
    let responders = vec![
        responder(
            [10, 77, 0, 2],
            ResponderClass::AddressOnly,
            [0, 0, 0, 0],
            None,
        ),
        responder(
            [10, 77, 0, 3],
            ResponderClass::PxeCapable,
            [10, 77, 0, 3],
            Some("x.efi"),
        ),
    ];
    let report = ProbeReport::assess(at, responders.clone(), &proxy, local, &[]);
    ensure_eq(
        &report.conflicts,
        &vec![Ipv4Addr::new(10, 77, 0, 3)],
        "伪造者为冲突",
    )?;
    let acked = ProbeReport::assess(
        at,
        responders,
        &proxy,
        local,
        &[Ipv4Addr::new(10, 77, 0, 3)],
    );
    ensure(acked.conflicts.is_empty(), "已确认的不再算冲突")?;
    Ok(())
}

/// 复用现网 DHCP 的四种检查结论。
///
/// # Errors
///
/// 断言失败。
#[test]
fn external_assessment() -> TestResult {
    let at = Timestamp::from_unix_seconds(1);
    let local = Ipv4Addr::new(10, 77, 0, 1);
    let external = BootNetwork::ExternalDhcp {
        iface: NetIface::parse("br-lab")?,
    };
    let cases = [
        (vec![], ExternalCheck::NoDhcp),
        (
            vec![responder(
                [10, 77, 0, 2],
                ResponderClass::AddressOnly,
                [0, 0, 0, 0],
                None,
            )],
            ExternalCheck::MissingNextServer,
        ),
        (
            vec![responder(
                [10, 77, 0, 2],
                ResponderClass::PxeCapable,
                [10, 77, 0, 1],
                Some("pxelinux.0"),
            )],
            ExternalCheck::FileMismatch,
        ),
        (
            vec![responder(
                [10, 77, 0, 2],
                ResponderClass::PxeCapable,
                [10, 77, 0, 1],
                Some("ipxe.efi"),
            )],
            ExternalCheck::Correct,
        ),
    ];
    for (list, want) in cases {
        let verdict = ProbeReport::assess(at, list, &external, local, &[]);
        ensure_eq(&verdict.external, &Some(want), "复用模式检查结论")?;
        ensure(verdict.conflicts.is_empty(), "复用模式不判冲突")?;
    }
    Ok(())
}

/// 健康检查：时效与自报状态。
///
/// # Errors
///
/// 断言失败。
#[test]
fn heartbeat_health() -> TestResult {
    let record: HeartbeatRecord = serde_json::from_str(
        r#"{"received_at":100,"heartbeat":{"at":100,"version":"0.1.0","rev":1,"mode":"proxy_dhcp","health":"ok","detail":null,
        "listening":[67,4011,69],"counters":{"dhcp_received":0,"dhcp_malformed":0,"dhcp_rate_limited":0,
        "dhcp_replies":0,"dhcp_ignored":0,"dhcp_paused":0,"tftp_completed":0,"tftp_failed":0},"probe":null}}"#,
    )?;
    ensure(
        record.is_healthy(Timestamp::from_unix_seconds(130), 30),
        "30 s 内健康",
    )?;
    ensure(
        !record.is_healthy(Timestamp::from_unix_seconds(131), 30),
        "超过 30 s 不健康",
    )?;
    let mut degraded = record;
    degraded.heartbeat.health = Health::Degraded;
    ensure(
        !degraded.is_healthy(Timestamp::from_unix_seconds(100), 30),
        "降级不健康",
    )?;
    Ok(())
}
