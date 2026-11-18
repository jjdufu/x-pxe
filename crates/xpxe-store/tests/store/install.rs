//! 装机会话、令牌、节点事件与版本固定。

use std::sync::Arc;
use std::thread;
use std::time::Duration;

use serde_json::json;
use xpxe_domain::{EventId, Hostname, MacAddr, NodeState, Timeouts, Ttl};
use xpxe_store::{
    EventInsert, EventRecord, NewNode, Pinned, Resource, ServedDoc, SessionRequest, Store,
    StoreError, TokenKind,
};
use xpxe_test_support::{TempDataDir, TestResult, ensure, ensure_eq};

use crate::support::{at, by, open};

/// 建一个节点并签发会话，返回 `(node_id, 会话)`。
///
/// # Errors
///
/// 写入失败时返回错误。
pub(crate) fn armed(store: &Store) -> TestResult<(i64, xpxe_store::IssuedSession)> {
    let configs = store.configs();
    let timeouts = configs.create(
        "default",
        &Timeouts::from_ttl(Ttl::standard()),
        &by("初版", 0),
    )?;
    let node = store.nodes().create(
        &NewNode {
            spec_object_id: timeouts.object_id,
            mac: MacAddr::parse("52:54:00:aa:bb:01")?,
            hostname: Hostname::parse("node01")?,
        },
        at(0),
    )?;
    let issued = store.tokens().issue(
        &SessionRequest {
            node_id: node.id,
            channel: "lan".to_owned(),
            pinned: Pinned {
                profile: timeouts,
                init_plan: None,
                asset_channels: None,
                canary_assets: Vec::new(),
            },
            ttl: Duration::from_hours(2),
        },
        at(0),
    )?;
    Ok((node.id, issued))
}

/// 两种令牌只能访问各自资源；过期与关闭会话被拒。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn token_kinds_are_isolated() -> TestResult {
    let dir = TempDataDir::new()?;
    let store = open(dir.path())?;
    let (_node, issued) = armed(&store)?;
    let tokens = store.tokens();
    let boot = issued.boot.as_str();
    let agent = issued.agent.as_str();
    ensure_eq(
        &tokens.resolve(boot, at(1))?,
        &Some((issued.session.clone(), TokenKind::Boot)),
        "boot 令牌",
    )?;
    ensure_eq(
        &tokens.resolve(agent, at(1))?,
        &Some((issued.session.clone(), TokenKind::Agent)),
        "agent 令牌",
    )?;
    ensure(
        tokens.authorize(boot, Resource::Preseed, at(1))?.is_some(),
        "boot→preseed",
    )?;
    ensure(
        tokens.authorize(boot, Resource::Plan, at(1))?.is_none(),
        "boot↛plan",
    )?;
    ensure(
        tokens.authorize(boot, Resource::Events, at(1))?.is_none(),
        "boot↛events",
    )?;
    ensure(
        tokens.authorize(agent, Resource::Plan, at(1))?.is_some(),
        "agent→plan",
    )?;
    ensure(
        tokens.authorize(agent, Resource::Kernel, at(1))?.is_none(),
        "agent↛kernel",
    )?;
    ensure(
        tokens.authorize(agent, Resource::Preseed, at(1))?.is_none(),
        "agent↛preseed",
    )?;
    ensure(
        tokens.resolve("unknown-token-value", at(1))?.is_none(),
        "未知令牌",
    )?;
    ensure(tokens.resolve(boot, at(2 * 3600))?.is_none(), "过期")?;
    ensure(tokens.close(&issued.session, "ready", at(10))?, "关闭")?;
    ensure(tokens.resolve(agent, at(11))?.is_none(), "关闭后失效")?;
    ensure_eq(
        &tokens.get(&issued.session)?.outcome,
        &Some("ready".to_owned()),
        "结果",
    )?;
    Ok(())
}

/// `consume_once` 在 16 个并发调用下只有一个成功。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn consume_once_is_atomic() -> TestResult {
    let dir = TempDataDir::new()?;
    let store = Arc::new(open(dir.path())?);
    let (_node, issued) = armed(&store)?;
    let mut handles = Vec::new();
    for _ in 0_u32..16 {
        let shared = Arc::clone(&store);
        let session = issued.session.clone();
        handles.push(thread::spawn(move || {
            shared
                .tokens()
                .consume_once(&session, ServedDoc::Preseed, at(5))
        }));
    }
    let mut wins = 0_u32;
    for handle in handles {
        match handle.join().map_err(|_panic| "线程崩溃")? {
            Ok(()) => wins = wins.saturating_add(1),
            Err(StoreError::AlreadyServed) => {}
            Err(other) => return Err(other.into()),
        }
    }
    ensure_eq(&wins, &1, "只有一个成功")?;
    store
        .tokens()
        .consume_once(&issued.session, ServedDoc::Plan, at(6))?;
    ensure_eq(
        &store
            .tokens()
            .consume_once(&issued.session, ServedDoc::Plan, at(7)),
        &Err(StoreError::AlreadyServed),
        "计划也只下发一次",
    )?;
    let session = store.tokens().get(&issued.session)?;
    ensure_eq(&session.preseed_served_at, &Some(at(5)), "preseed 时间")?;
    ensure_eq(&session.plan_served_at, &Some(at(6)), "计划时间")?;
    Ok(())
}

/// 会话固定后修改配置不影响会话所固定的版本内容。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn pinned_versions_survive_edits() -> TestResult {
    let dir = TempDataDir::new()?;
    let store = open(dir.path())?;
    let (_node, issued) = armed(&store)?;
    let pinned = store.tokens().get(&issued.session)?.pinned.profile;
    let changed = Timeouts {
        arm: 60,
        ..Timeouts::from_ttl(Ttl::standard())
    };
    let head = store
        .configs()
        .save(pinned.object_id, &changed, pinned.version, &by("缩短", 1))?;
    ensure_eq(&head.version, &2, "配置已改")?;
    let again = store.tokens().get(&issued.session)?.pinned.profile;
    ensure_eq(&again, &pinned, "会话仍固定版本 1")?;
    let doc: Timeouts = store.configs().load(again.object_id, again.version)?;
    ensure_eq(&doc, &Timeouts::from_ttl(Ttl::standard()), "固定内容不变")?;
    Ok(())
}

/// 事件按 `event_id` 幂等；状态不符被拒；运行态随事件更新。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn node_events_are_idempotent() -> TestResult {
    let dir = TempDataDir::new()?;
    let store = open(dir.path())?;
    let (node_id, issued) = armed(&store)?;
    let event = EventRecord {
        event_id: EventId::parse("admin.arm.1")?,
        session_id: Some(issued.session.clone()),
        at: at(3),
        source: "admin".to_owned(),
        kind: "arm".to_owned(),
        from_state: NodeState::Idle,
        to_state: NodeState::Armed,
        deadline_at: Some(at(7203)),
        active_session_id: Some(issued.session.clone()),
        payload: json!({}),
    };
    let nodes = store.nodes();
    ensure(
        matches!(nodes.record(node_id, &event)?, EventInsert::Inserted(_)),
        "首次写入",
    )?;
    ensure_eq(
        &nodes.record(node_id, &event)?,
        &EventInsert::Duplicate,
        "重复",
    )?;
    let row = nodes.get(node_id)?;
    ensure_eq(&row.state, &NodeState::Armed, "状态")?;
    ensure_eq(&row.state_since, &at(3), "进入时间")?;
    ensure_eq(&row.active_session_id, &Some(issued.session), "会话")?;
    let stale = EventRecord {
        event_id: EventId::parse("admin.arm.2")?,
        ..event
    };
    ensure_eq(
        &nodes.record(node_id, &stale),
        &Err(StoreError::StateChanged {
            current: NodeState::Armed,
        }),
        "状态不符",
    )?;
    ensure_eq(&nodes.events(node_id, 10)?.len(), &1, "只有一条事件")?;
    ensure_eq(
        &nodes
            .by_mac(MacAddr::parse("52:54:00:aa:bb:01")?)?
            .map(|found| found.id),
        &Some(node_id),
        "按 MAC 查",
    )?;
    ensure_eq(&nodes.list()?.len(), &1, "列表")?;
    Ok(())
}
