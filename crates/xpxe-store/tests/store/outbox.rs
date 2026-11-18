//! 原子提交（事件 + 审计 + 发件箱）与发件箱重试。

use serde_json::json;
use xpxe_domain::{EventId, NodeState};
use xpxe_store::{AuditRecord, Commit, EventInsert, EventRecord, OutboxItem};
use xpxe_test_support::{TempDataDir, TestResult, ensure, ensure_eq};

use crate::install::armed;
use crate::support::{at, open};

/// 提交一次写入事件、审计与副作用；重复提交不再写；发件箱可延期与完成。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn commit_is_atomic_and_outbox_retries() -> TestResult {
    let dir = TempDataDir::new()?;
    let store = open(dir.path())?;
    let (node_id, issued) = armed(&store)?;
    let change = Commit {
        event: EventRecord {
            event_id: EventId::parse("admin.arm.1")?,
            session_id: Some(issued.session.clone()),
            at: at(5),
            source: "admin".to_owned(),
            kind: "arm".to_owned(),
            from_state: NodeState::Idle,
            to_state: NodeState::Armed,
            deadline_at: None,
            active_session_id: Some(issued.session.clone()),
            payload: json!({}),
        },
        audit: vec![AuditRecord {
            actor: "admin".to_owned(),
            ip: "127.0.0.1".to_owned(),
            action: "node.arm".to_owned(),
            target: format!("node:{node_id}"),
            outcome: "ok".to_owned(),
            detail: json!({}),
        }],
        outbox: vec![OutboxItem {
            node_id,
            session_id: Some(issued.session),
            effect: "publish_directives".to_owned(),
            payload: json!({}),
        }],
    };
    ensure(
        matches!(
            store.nodes().commit(node_id, &change)?,
            EventInsert::Inserted(_)
        ),
        "首次提交",
    )?;
    ensure_eq(
        &store.nodes().commit(node_id, &change)?,
        &EventInsert::Duplicate,
        "重复提交",
    )?;
    ensure_eq(&store.audit().verify()?.checked, &1, "只有一条审计")?;
    let outbox = store.outbox();
    let due = outbox.due(at(5), 10)?;
    ensure_eq(&due.len(), &1, "一条副作用")?;
    let first = due.first().ok_or("缺少条目")?;
    outbox.defer(first.id, "失败", at(9))?;
    ensure(outbox.due(at(8), 10)?.is_empty(), "退避期间不执行")?;
    ensure_eq(&outbox.next_due()?, &Some(at(9)), "下次时间")?;
    let again = outbox.due(at(9), 10)?;
    ensure_eq(
        &again.first().map(|entry| entry.attempts),
        &Some(1),
        "尝试次数",
    )?;
    outbox.complete(first.id, at(9))?;
    ensure_eq(&outbox.next_due()?, &None, "全部完成")?;
    Ok(())
}
