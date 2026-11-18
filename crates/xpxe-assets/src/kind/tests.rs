//! 资产种类与频道文档单元测试。

use std::collections::BTreeMap;

use xpxe_domain::ConfigDocument as _;
use xpxe_test_support::{TestResult, ensure, ensure_eq};

use super::{ALL_KINDS, AssetChannels, AssetKind, ChannelEntry, parse_kind};

/// 名称往返、官方 URL 与上限。
///
/// # Errors
///
/// 结果不符时返回错误。
#[test]
fn kind_names() -> TestResult {
    for kind in ALL_KINDS {
        ensure_eq(&parse_kind(kind.as_str())?, &kind, "roundtrip")?;
    }
    ensure(parse_kind("agent").is_err(), "agent is not downloadable")?;
    ensure_eq(
        &AssetKind::IpxeEfiX64.official_url(),
        &Some("https://boot.ipxe.org/x86_64-efi/ipxe.efi"),
        "efi url",
    )?;
    ensure_eq(
        &AssetKind::IpxeUndionly.size_limit(),
        &(4 * 1024 * 1024),
        "ipxe limit",
    )?;
    Ok(())
}

/// 频道文档序列化与校验。
///
/// # Errors
///
/// 结果不符时返回错误。
#[test]
fn channels_doc() -> TestResult {
    let mut channels = BTreeMap::new();
    let _none = channels.insert(
        AssetKind::IpxeEfiX64,
        ChannelEntry {
            asset_id: 3,
            sha256: "a".repeat(64),
        },
    );
    let doc = AssetChannels { channels };
    ensure(doc.validate().is_ok(), "valid")?;
    ensure(doc.references(3) && !doc.references(4), "references")?;
    let json = serde_json::to_value(&doc)?;
    ensure(
        json.pointer("/channels/ipxe_efi_x64/asset_id").is_some(),
        "snake keys",
    )?;
    let back = AssetChannels::migrate(1, json)?;
    ensure_eq(&back, &doc, "migrate current")?;
    let mut bad = doc;
    if let Some(entry) = bad.channels.get_mut(&AssetKind::IpxeEfiX64) {
        entry.sha256 = "zz".to_owned();
        entry.asset_id = 0;
    }
    ensure_eq(
        &bad.validate().err().map(|errors| errors.len()),
        &Some(2),
        "two errors",
    )?;
    Ok(())
}
