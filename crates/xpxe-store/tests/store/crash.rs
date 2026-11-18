//! 崩溃安全、备份、资产与观测。

use rusqlite::Connection;
use serde_json::json;
use xpxe_domain::{Timeouts, Ttl};
use xpxe_store::{AssetSource, AssetStage, DB_FILE, NewAsset, Observation, RETENTION, Store};
use xpxe_test_support::{TempDataDir, TestResult, ensure, ensure_eq};

use crate::support::{at, by, key_a, open};

/// 事务中途丢弃连接（未提交）后，重新打开数据库一致且看不到半截写入。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn dropped_transaction_leaves_db_consistent() -> TestResult {
    let dir = TempDataDir::new()?;
    let object_id = {
        let store = open(dir.path())?;
        store
            .configs()
            .create("t", &Timeouts::from_ttl(Ttl::standard()), &by("初版", 0))?
            .object_id
    };
    {
        let mut raw = Connection::open(dir.path().join(DB_FILE))?;
        let tx = raw.transaction()?;
        let _inserted = tx.execute(
            "INSERT INTO config_versions (object_id, version, author_id, created_at, comment, schema, content, content_sha256)
             VALUES (?1, 2, NULL, '2026-11-18T00:00:00Z', 'half', 1, '{}', 'x')",
            [object_id],
        )?;
        let _moved_head = tx.execute(
            "UPDATE config_objects SET head_version = 2 WHERE id = ?1",
            [object_id],
        )?;
        // 模拟进程在提交前崩溃：事务与连接直接丢弃。
        drop(tx);
        drop(raw);
    }
    let store = open(dir.path())?;
    ensure(store.integrity_check()?.is_empty(), "完整性")?;
    let (head, _doc) = store.configs().head::<Timeouts>(object_id)?;
    ensure_eq(&head.version, &1, "半截写入未生效")?;
    ensure_eq(&store.configs().versions(object_id)?.len(), &1, "历史一条")?;
    Ok(())
}

/// 备份可独立打开并通过完整性与审计校验。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn backup_opens_independently() -> TestResult {
    let dir = TempDataDir::new()?;
    let store = open(&dir.path().join("live"))?;
    let _created =
        store
            .configs()
            .create("t", &Timeouts::from_ttl(Ttl::standard()), &by("初版", 0))?;
    let _audit_id = store.audit().append(
        &xpxe_store::AuditRecord {
            actor: "system".to_owned(),
            ip: "-".to_owned(),
            action: "backup".to_owned(),
            target: "db".to_owned(),
            outcome: "ok".to_owned(),
            detail: json!({}),
        },
        at(1),
    )?;
    let restored = dir.path().join("restored");
    std::fs::create_dir_all(&restored)?;
    store.backup(&restored.join(DB_FILE))?;
    ensure(store.backup(&restored.join(DB_FILE)).is_err(), "目标已存在")?;
    let copy = Store::open(&restored, key_a())?;
    ensure(copy.integrity_check()?.is_empty(), "备份完整")?;
    ensure_eq(&copy.audit().verify()?.first_broken, &None, "审计链完整")?;
    ensure_eq(
        &copy.schema_version()?,
        &store.schema_version()?,
        "schema 一致",
    )?;
    Ok(())
}

/// 资产元数据登记与阶段变化。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn asset_metadata() -> TestResult {
    let dir = TempDataDir::new()?;
    let store = open(dir.path())?;
    let assets = store.assets();
    let asset = NewAsset {
        kind: "kernel".to_owned(),
        label: "debian-13 linux".to_owned(),
        source: AssetSource::Official,
        source_url: Some("https://deb.debian.org/debian/".to_owned()),
        sha256: "ab".repeat(32),
        size: 12_345,
        verify: json!({ "signed_by": "debian" }),
        kernel_abi: Some("6.12.0-1-amd64".to_owned()),
        created_by: "admin".to_owned(),
    };
    let id = assets.insert(&asset, at(0))?;
    let bad = NewAsset {
        sha256: "zz".to_owned(),
        ..asset
    };
    ensure(assets.insert(&bad, at(0)).is_err(), "sha256 格式")?;
    assets.set_stage(id, AssetStage::Verified, Some(&json!({ "ok": true })))?;
    let record = assets.get(id)?;
    ensure_eq(&record.stage, &AssetStage::Verified, "阶段")?;
    ensure_eq(&record.meta.verify, &json!({ "ok": true }), "结论")?;
    ensure_eq(&record.meta.size, &12_345, "大小")?;
    ensure_eq(&assets.list()?.len(), &1, "列表")?;
    ensure(
        assets.set_stage(999, AssetStage::Retired, None).is_err(),
        "不存在",
    )?;
    Ok(())
}

/// 观测滚动保留 30 天，并能列出未登记 MAC。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn observations_roll_over() -> TestResult {
    let dir = TempDataDir::new()?;
    let store = open(dir.path())?;
    let observations = store.observations();
    for (secs, mac) in [(0, "52:54:00:00:00:01"), (86_400, "52:54:00:00:00:02")] {
        observations.record(&Observation {
            at: at(secs),
            mac: mac.to_owned(),
            arch: "uefi_x64".to_owned(),
            user_class: String::new(),
            kind: "dhcp_ignore".to_owned(),
            detail: "unknown mac".to_owned(),
        })?;
    }
    ensure_eq(&observations.unknown_macs()?.len(), &2, "两个未登记 MAC")?;
    let removed = observations.prune(at(RETENTION.as_secs().saturating_add(10)))?;
    ensure_eq(&removed, &1, "删除超期一条")?;
    ensure_eq(&observations.recent(10)?.len(), &1, "剩一条")?;
    Ok(())
}
