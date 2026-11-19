//! 回放用例：所有夹具 MAC 均已武装（U6 的 c6/c7 按节点覆盖为 `snponly.efi`），
//! 比较 `yiaddr`、`siaddr`、`file`、选项 43/54/60/97 与目的地址。
//!
//! 设置 `XPXE_REPLAY_BLESS=1` 时重写期望文件（仅在人工审阅差异后使用）。

use std::fmt::Write as _;
use std::fs;
use std::net::{Ipv4Addr, SocketAddrV4};
use std::path::{Path, PathBuf};
use std::time::Instant;

use serde_json::{Value, json};
use xpxe_bootnet::answering::{Context, Engine, Inbound, Verdict};
use xpxe_dhcp::wire::{Message, code};
use xpxe_domain::{
    BootNetwork, BootToken, DhcpRules, DirectiveSnapshot, FirmwareSet, HttpUrl, MacAddr, NetIface,
    SessionId, Sha256Hex, SnapshotEntry, Timestamp, UefiFirmware,
};
use xpxe_test_support::{TestFailure, TestResult, ensure, ensure_eq};

/// 夹具目录。
fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../xpxe-dhcp/tests/fixtures")
}

/// 期望目录。
fn expectations() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/replay/expect")
}

/// 构造武装全部夹具 MAC 的快照。
///
/// # Errors
///
/// 值解析失败。
fn snapshot(macs: &[MacAddr]) -> TestResult<DirectiveSnapshot> {
    let set = FirmwareSet {
        undionly: Sha256Hex::parse(&"a1".repeat(32))?,
        ipxe_efi: Sha256Hex::parse(&"b2".repeat(32))?,
        snponly: Sha256Hex::parse(&"c3".repeat(32))?,
    };
    let snponly = [
        MacAddr::parse("52:54:00:77:01:c6")?,
        MacAddr::parse("52:54:00:77:01:c7")?,
    ];
    let directives = macs
        .iter()
        .map(|mac| {
            Ok(SnapshotEntry {
                mac: *mac,
                session: SessionId::parse("replay")?,
                boot_token: BootToken::parse(&format!("tok-{}", mac.to_hexhyp()))?,
                firmware: set.clone(),
                uefi_firmware: snponly.contains(mac).then_some(UefiFirmware::SnpOnlyEfi),
                expires_at: Timestamp::from_unix_seconds(u64::MAX),
            })
        })
        .collect::<TestResult<Vec<_>>>()?;
    Ok(DirectiveSnapshot {
        rev: 1,
        network: BootNetwork::ProxyDhcp {
            iface: NetIface::parse("br-lab")?,
        },
        rules: DhcpRules::standard(),
        boot_base_url: HttpUrl::parse("http://10.77.0.1:8080")?,
        active_firmware: Some(set),
        directives,
        acknowledged_conflicts: Vec::new(),
        probe_nonce: 0,
    })
}

/// 字节的十六进制文本。
fn hex_text(bytes: &[u8]) -> String {
    bytes.iter().fold(
        String::with_capacity(bytes.len().saturating_mul(2)),
        |mut text, byte| {
            // 写入 String 不会失败。
            let _ignored = write!(text, "{byte:02x}");
            text
        },
    )
}

/// 选项的十六进制文本。
fn hex_option(reply: &Message, option: u8) -> Value {
    reply
        .option(option)
        .map_or(Value::Null, |value| Value::String(hex_text(value)))
}
/// 选项的文本。
fn text_option(reply: &Message, option: u8) -> Value {
    reply.option(option).map_or(Value::Null, |value| {
        Value::String(String::from_utf8_lossy(value).into_owned())
    })
}

/// 把处理结论转为可比较的 JSON。
///
/// # Errors
///
/// 应答无法解析。
fn describe(verdict: Verdict) -> TestResult<Value> {
    Ok(match verdict {
        Verdict::Replied { bytes, to, .. } => {
            let reply = Message::decode(&bytes)?;
            json!({
                "to": to.to_string(),
                "yiaddr": reply.yiaddr.to_string(),
                "siaddr": reply.siaddr.to_string(),
                "file": String::from_utf8_lossy(&reply.file),
                "opt43": hex_option(&reply, code::VENDOR_SPECIFIC),
                "opt54": hex_option(&reply, code::SERVER_ID),
                "opt60": text_option(&reply, code::VENDOR_CLASS),
                "opt97": hex_option(&reply, code::CLIENT_UUID),
            })
        }
        Verdict::Ignored(observation) => {
            json!({ "ignored": serde_json::to_value(*observation)?.get("reason") })
        }
        Verdict::Unrelated => json!({ "unrelated": true }),
        Verdict::Dropped(counter) => json!({ "dropped": format!("{counter:?}") }),
    })
}

/// 一个 DISCOVER 夹具。
struct Fixture {
    /// 文件名。
    name: String,
    /// 报文。
    bytes: Vec<u8>,
    /// 客户端 MAC。
    mac: MacAddr,
    /// 是否为 d-i 安装器自身的 DHCP（Opt60 = `d-i`，必须静默）。
    installer: bool,
}

/// 读取全部 DISCOVER 夹具（按名称排序）。
///
/// # Errors
///
/// 读取或解析失败。
fn discover_fixtures() -> TestResult<Vec<Fixture>> {
    let mut found = Vec::new();
    for entry in fs::read_dir(fixtures())? {
        let path = entry?.path();
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_owned();
        let is_bin = path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("bin"));
        if !(is_bin && name.contains("-discover-67")) {
            continue;
        }
        let bytes = fs::read(&path)?;
        let message = Message::decode(&bytes)?;
        let mac = message
            .mac()
            .ok_or_else(|| TestFailure::new(format!("{name} 无 MAC")))?;
        let installer = message.option(code::VENDOR_CLASS) == Some(b"d-i".as_slice());
        found.push(Fixture {
            mac: MacAddr::parse(&hex_text(&mac))?,
            name,
            bytes,
            installer,
        });
    }
    found.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(found)
}

/// 全部 DISCOVER 夹具回放并与期望一致。
///
/// # Errors
///
/// 任一夹具应答与期望不符。
#[test]
fn replay_matches_expectations() -> TestResult {
    let found = discover_fixtures()?;
    let macs: Vec<MacAddr> = found.iter().map(|fixture| fixture.mac).collect();
    ensure(found.len() >= 30, "至少 30 个 DISCOVER 夹具")?;
    let snap = snapshot(&macs)?;
    let bless = std::env::var_os("XPXE_REPLAY_BLESS").is_some();
    for fixture in &found {
        let mut engine = Engine::new(Instant::now());
        let ctx = Context {
            snapshot: &snap,
            server: Ipv4Addr::new(10, 77, 0, 1),
            paused: false,
            now: Instant::now(),
            wall: Timestamp::from_unix_seconds(1_800_000_000),
        };
        let inbound = Inbound {
            payload: &fixture.bytes,
            port: 67,
            from: SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 68),
        };
        let actual = describe(engine.handle(inbound, &ctx))?;
        ensure_eq(
            &actual.get("yiaddr").is_some(),
            &!fixture.installer,
            &fixture.name,
        )?;
        let expect_path = expectations().join(fixture.name.replace(".bin", ".expect.json"));
        if bless {
            fs::write(
                &expect_path,
                format!("{}\n", serde_json::to_string_pretty(&actual)?),
            )?;
        }
        let expected: Value = serde_json::from_slice(&fs::read(&expect_path)?)?;
        ensure_eq(&actual, &expected, &fixture.name)?;
    }
    Ok(())
}
