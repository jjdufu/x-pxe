//! 审计哈希链。

use rusqlite::Connection;
use serde_json::json;
use xpxe_store::{AuditRecord, ChainReport, DB_FILE};
use xpxe_test_support::{TempDataDir, TestResult, ensure, ensure_eq};

use crate::support::{at, open};

/// 构造审计记录。
fn record(action: &str, n: u32) -> AuditRecord {
    AuditRecord {
        actor: "admin".to_owned(),
        ip: "10.77.0.1".to_owned(),
        action: action.to_owned(),
        target: format!("node{n}"),
        outcome: "ok".to_owned(),
        detail: json!({ "n": n }),
    }
}

/// 完整链校验通过；直接改 SQLite 的任意一行后 `verify` 指出该行。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn tamper_is_located() -> TestResult {
    let dir = TempDataDir::new()?;
    let store = open(dir.path())?;
    for n in 1..=5 {
        let _id = store.audit().append(&record("arm", n), at(n.into()))?;
    }
    ensure_eq(
        &store.audit().verify()?,
        &ChainReport {
            checked: 5,
            first_broken: None,
        },
        "完整",
    )?;
    let raw = Connection::open(dir.path().join(DB_FILE))?;
    let _tampered = raw.execute(
        "UPDATE audit_log SET detail = '{\"n\":99}' WHERE id = 3",
        [],
    )?;
    ensure_eq(&store.audit().verify()?.first_broken, &Some(3), "改内容")?;
    let _restored = raw.execute("UPDATE audit_log SET detail = '{\"n\":3}' WHERE id = 3", [])?;
    ensure_eq(&store.audit().verify()?.first_broken, &None, "恢复后完整")?;
    let _deleted = raw.execute("DELETE FROM audit_log WHERE id = 2", [])?;
    ensure_eq(&store.audit().verify()?.first_broken, &Some(3), "删行")?;
    Ok(())
}

/// 导出为 JSON Lines，按时间过滤。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn export_json_lines() -> TestResult {
    let dir = TempDataDir::new()?;
    let store = open(dir.path())?;
    for n in 1..=4 {
        let _id = store
            .audit()
            .append(&record("login", n), at(u64::from(n) * 60))?;
    }
    let mut out = Vec::new();
    let count = store.audit().export_since(at(180), &mut out)?;
    ensure_eq(&count, &2, "两条")?;
    let text = String::from_utf8(out)?;
    let lines: Vec<serde_json::Value> = text
        .lines()
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()?;
    ensure_eq(&lines.len(), &2, "两行")?;
    ensure(
        lines.iter().all(|line| {
            line.get("hash")
                .and_then(serde_json::Value::as_str)
                .is_some()
        }),
        "带哈希",
    )?;
    Ok(())
}
