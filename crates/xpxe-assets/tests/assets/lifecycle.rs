//! 资产生命周期：未过金丝雀激活被拒；跳过金丝雀需理由并写审计；回滚；被引用资产不可删除。

use std::time::Duration;

use serde_json::json;
use xpxe_assets::{Actor, AssetKind, CanaryRun, Catalog, Operation, VerifiedAsset};
use xpxe_domain::{Hostname, MacAddr, Timeouts, Timestamp, Ttl};
use xpxe_store::{
    AssetSource, AssetStage, Authorship, MasterKey, NewNode, Pinned, Role, SessionRequest, Store,
};
use xpxe_test_support::{TempDataDir, TestResult, ensure, ensure_eq};

use crate::support::{NOW, code};

/// 操作者。
fn actor(role: Role) -> Actor {
    Actor {
        user_id: None,
        name: format!("{}-user", role.as_str()),
        role,
        ip: "127.0.0.1".to_owned(),
    }
}

/// 写一个假 iPXE 文件并构造待登记资产。
///
/// # Errors
///
/// 写文件失败时返回错误。
fn fake_ipxe(dir: &std::path::Path, content: &[u8]) -> TestResult<VerifiedAsset> {
    let sha256 = hex::encode(<sha2::Sha256 as sha2::Digest>::digest(content));
    std::fs::write(dir.join(&sha256), content)?;
    Ok(VerifiedAsset {
        kind: AssetKind::IpxeEfiX64,
        label: "ipxe.efi test".to_owned(),
        source: AssetSource::Upload,
        source_url: None,
        sha256,
        size: u64::try_from(content.len())?,
        kernel_abi: None,
        verify: json!({ "format": "test" }),
    })
}

/// 激活前的各种拒绝。
///
/// # Errors
///
/// 断言失败时返回错误。
fn check_denials(catalog: &Catalog<'_>, asset_id: i64, op: Operation<'_>) -> TestResult {
    let denied = catalog.activate(AssetKind::IpxeEfiX64, asset_id, None, op);
    ensure_eq(&code(&denied), &Some("canary_required"), "no canary")?;
    let blank = catalog.activate(AssetKind::IpxeEfiX64, asset_id, Some("  "), op);
    ensure_eq(&code(&blank), &Some("canary_required"), "blank reason")?;
    let operator = actor(Role::Operator);
    let forbidden = catalog.activate(
        AssetKind::IpxeEfiX64,
        asset_id,
        Some("x"),
        Operation {
            actor: &operator,
            now: NOW,
        },
    );
    ensure_eq(&code(&forbidden), &Some("forbidden"), "operator")?;
    let wrong_kind = catalog.activate(AssetKind::IpxeUndionly, asset_id, Some("x"), op);
    ensure_eq(&code(&wrong_kind), &Some("kind_mismatch"), "kind")?;

    Ok(())
}

/// 激活、金丝雀、回滚与删除保护。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn activation_rollback_delete() -> TestResult {
    let data = TempDataDir::new()?;
    let store = Store::open(data.path(), MasterKey::from_bytes([3; 32]))?;
    let assets_dir = data.path().join("assets");
    std::fs::create_dir_all(&assets_dir)?;
    let catalog = Catalog::new(&store, &assets_dir);
    let admin = actor(Role::Admin);
    let op = Operation {
        actor: &admin,
        now: NOW,
    };
    let first = catalog.register(&fake_ipxe(&assets_dir, b"one")?, op)?;
    let second = catalog.register(&fake_ipxe(&assets_dir, b"two")?, op)?;
    ensure_eq(&first.stage, &AssetStage::Verified, "verified")?;

    check_denials(&catalog, first.id, op)?;

    let v1 = catalog.activate(AssetKind::IpxeEfiX64, first.id, Some("实验室首次导入"), op)?;
    catalog.mark_canary_passed(
        second.id,
        &CanaryRun {
            node_id: 1,
            session_id: "s1".to_owned(),
        },
        op,
    )?;
    let v2 = catalog.activate(AssetKind::IpxeEfiX64, second.id, None, op)?;
    ensure(v2.version > v1.version, "new version")?;

    let mut export = Vec::new();
    let _rows = store
        .audit()
        .export_since(Timestamp::from_unix_seconds(0), &mut export)?;
    let audit = String::from_utf8(export)?;
    ensure(audit.contains("实验室首次导入"), "skip reason audited")?;
    ensure(audit.contains("\"denied\""), "denial audited")?;

    ensure_eq(
        &code(&catalog.delete(second.id, op)),
        &Some("in_use"),
        "active asset",
    )?;
    let back = catalog.rollback(AssetKind::IpxeEfiX64, op)?;
    let (_head, doc) = catalog.channels()?;
    ensure_eq(
        &doc.channels
            .get(&AssetKind::IpxeEfiX64)
            .map(|entry| entry.asset_id),
        &Some(first.id),
        "rolled back",
    )?;
    ensure(back.version > v2.version, "rollback is a new version")?;

    catalog.delete(second.id, op)?;
    ensure(
        !catalog.path_of(&second.meta.sha256).exists(),
        "file removed",
    )?;
    ensure(
        catalog.path_of(&first.meta.sha256).exists(),
        "active file kept",
    )?;
    let again = catalog.rollback(AssetKind::IpxeEfiX64, op);
    ensure(again.is_ok(), "rollback to unset allowed")?;
    Ok(())
}

/// 进行中会话固定的频道版本与金丝雀资产阻止删除。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn pinned_assets_are_protected() -> TestResult {
    let data = TempDataDir::new()?;
    let store = Store::open(data.path(), MasterKey::from_bytes([4; 32]))?;
    let assets_dir = data.path().join("assets");
    std::fs::create_dir_all(&assets_dir)?;
    let catalog = Catalog::new(&store, &assets_dir);
    let admin = actor(Role::Admin);
    let op = Operation {
        actor: &admin,
        now: NOW,
    };
    let pinned = catalog.register(&fake_ipxe(&assets_dir, b"pinned")?, op)?;
    let canary = catalog.register(&fake_ipxe(&assets_dir, b"canary")?, op)?;
    let replacement = catalog.register(&fake_ipxe(&assets_dir, b"next")?, op)?;
    let channels = catalog.activate(AssetKind::IpxeEfiX64, pinned.id, Some("测试"), op)?;
    let _v2 = catalog.activate(AssetKind::IpxeEfiX64, replacement.id, Some("测试"), op)?;

    let meta = Authorship {
        author_id: None,
        comment: "初版".to_owned(),
        at: NOW,
    };
    let timeouts =
        store
            .configs()
            .create("default", &Timeouts::from_ttl(Ttl::standard()), &meta)?;
    let node = store.nodes().create(
        &NewNode {
            spec_object_id: timeouts.object_id,
            mac: MacAddr::parse("52:54:00:aa:bb:02")?,
            hostname: Hostname::parse("node02")?,
        },
        NOW,
    )?;
    let session = store.tokens().issue(
        &SessionRequest {
            node_id: node.id,
            channel: "lan".to_owned(),
            pinned: Pinned {
                profile: timeouts,
                init_plan: None,
                asset_channels: Some(channels),
                canary_assets: vec![canary.id],
            },
            ttl: Duration::from_hours(2),
        },
        NOW,
    )?;
    ensure_eq(
        &code(&catalog.delete(pinned.id, op)),
        &Some("in_use"),
        "pinned channel",
    )?;
    ensure_eq(
        &code(&catalog.delete(canary.id, op)),
        &Some("in_use"),
        "canary",
    )?;
    let _closed = store.tokens().close(&session.session, "ready", NOW)?;
    catalog.delete(pinned.id, op)?;
    catalog.delete(canary.id, op)?;
    Ok(())
}
