//! 统一的状态转换路径：管理员、安装器、agent 与定时器事件都经此提交。
//!
//! 读取持久化运行态 → `apply` → 同一事务写事件、运行态、审计与发件箱 → 推送并唤醒 worker。

use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};
use xpxe_domain::{
    AuditKind, Effect, EventEnvelope, EventId, Hostname, MacAddr, NodeRuntime, NodeState,
    Outcome as SessionOutcome, Persisted, Timeouts, Ttl, apply,
};
use xpxe_store::{AuditRecord, Commit, EventInsert, EventRecord, NodeRow, OutboxItem};

use crate::app::App;
use crate::auth::Actor;
use crate::configs::singleton;
use crate::{Failure, Outcome, internal};

/// 待投递的事件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Incoming {
    /// 节点。
    pub(crate) node_id: i64,
    /// 事件信封。
    pub(crate) envelope: EventEnvelope,
    /// 审计操作者。
    pub(crate) actor: Actor,
    /// 附加数据（写入节点事件）。
    pub(crate) payload: Value,
}

/// 投递结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Delivered {
    /// 转换前状态。
    pub(crate) from: NodeState,
    /// 转换后状态。
    pub(crate) to: NodeState,
    /// 是否为重复事件（未改动任何东西）。
    pub(crate) duplicate: bool,
}

/// 当前超时配置。
///
/// # Errors
///
/// 读取失败时返回错误。
pub(crate) fn current_ttl(app: &App) -> Outcome<Ttl> {
    Ok(singleton::<Timeouts>(&app.store)?.map_or_else(Ttl::standard, |(_, doc)| doc.to_ttl()))
}

/// 由数据库行恢复运行态。
///
/// # Errors
///
/// 主机名或 MAC 损坏时返回 500。
pub(crate) fn runtime_of(row: &NodeRow) -> Outcome<NodeRuntime> {
    let hostname = Hostname::parse(&row.hostname).map_err(internal)?;
    let mac = MacAddr::parse(&row.mac).map_err(internal)?;
    Ok(NodeRuntime::restore(
        (hostname, mac),
        Persisted {
            state: row.state,
            since: row.state_since,
            deadline: row.deadline_at,
            session: row.active_session_id.clone(),
        },
    ))
}

/// 生成唯一事件 ID：`<前缀>.<节点>.<时间>.<序号>`。
///
/// # Errors
///
/// 失败时返回错误。
pub(crate) fn fresh_id(app: &App, prefix: &str, node_id: i64) -> Outcome<EventId> {
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    EventId::parse(&format!(
        "{prefix}.{node_id}.{}.{seq}",
        app.now().unix_seconds()
    ))
    .map_err(internal)
}

/// 事件 ID 序号。
static SEQ: AtomicU64 = AtomicU64::new(1);

/// 审计动作名。
const fn audit_action(kind: AuditKind) -> &'static str {
    match kind {
        AuditKind::Armed => "node.armed",
        AuditKind::Disarmed => "node.disarmed",
        AuditKind::InstallStarted => "node.install_started",
        AuditKind::InstallFinished => "node.install_finished",
        AuditKind::AgentStarted => "node.agent_started",
        AuditKind::Ready => "node.ready",
        AuditKind::Failed => "node.failed",
        AuditKind::Reset => "node.reset",
    }
}

/// 会话结局名。
pub(crate) const fn outcome_name(outcome: SessionOutcome) -> &'static str {
    match outcome {
        SessionOutcome::Success => "success",
        SessionOutcome::Failure => "failure",
        SessionOutcome::Cancelled => "cancelled",
        SessionOutcome::Timeout => "timeout",
    }
}

/// 把副作用拆成审计与发件箱条目。
fn split_effects(
    incoming: &Incoming,
    row: &NodeRow,
    effects: &[Effect],
) -> (Vec<AuditRecord>, Vec<OutboxItem>) {
    let target = format!("node:{}", row.id);
    let mut audit = Vec::new();
    let mut outbox = Vec::new();
    let item = |effect: &str, payload: Value| OutboxItem {
        node_id: row.id,
        session_id: row.active_session_id.clone(),
        effect: effect.to_owned(),
        payload,
    };
    for effect in effects {
        match effect {
            Effect::Audit(kind) => audit.push(incoming.actor.record(
                audit_action(*kind),
                &target,
                "ok",
                json!({ "event": incoming.envelope.event.kind().as_str(), "hostname": row.hostname }),
            )),
            Effect::PublishDirectives => outbox.push(item("publish_directives", json!({}))),
            Effect::EjectMedia => outbox.push(item("eject_media", json!({}))),
            Effect::CloseSession(outcome) => {
                outbox.push(item("close_session", json!({ "outcome": outcome_name(*outcome) })));
            }
            Effect::OpenSession => {}
        }
    }
    (audit, outbox)
}

/// 事件是否已处理过（按 `event_id` 查最近事件）。
///
/// # Errors
///
/// 读取失败时返回错误。
fn seen(app: &App, node_id: i64, incoming: &Incoming) -> Outcome<bool> {
    Ok(app
        .store
        .nodes()
        .events(node_id, 200)?
        .iter()
        .any(|event| event.event_id == incoming.envelope.id.as_str()))
}

/// 投递事件。
///
/// # Errors
///
/// 节点不存在（404）、转换被拒（409，含 `STALE_SESSION`）或写入失败时返回错误。
pub(crate) fn deliver(app: &App, incoming: &Incoming) -> Outcome<Delivered> {
    let row = app.store.nodes().get(incoming.node_id)?;
    if seen(app, row.id, incoming)? {
        return Ok(Delivered {
            from: row.state,
            to: row.state,
            duplicate: true,
        });
    }
    let runtime = runtime_of(&row)?;
    let now = app.now();
    let transition = apply(&runtime, &incoming.envelope, now, &current_ttl(app)?)?;
    let next = runtime.commit(&incoming.envelope, now, &transition);
    let (audit, outbox) = split_effects(incoming, &row, &transition.effects);
    let change = Commit {
        event: EventRecord {
            event_id: incoming.envelope.id.clone(),
            session_id: incoming
                .envelope
                .session
                .clone()
                .or_else(|| row.active_session_id.clone()),
            at: now,
            source: format!("{:?}", incoming.envelope.source).to_lowercase(),
            kind: incoming.envelope.event.kind().as_str().to_owned(),
            from_state: row.state,
            to_state: transition.to,
            deadline_at: transition.deadline,
            active_session_id: next.session().cloned(),
            payload: incoming.payload.clone(),
        },
        audit,
        outbox,
    };
    let inserted = app.store.nodes().commit(row.id, &change)?;
    let duplicate = matches!(inserted, EventInsert::Duplicate);
    if !duplicate {
        app.push(
            "node.state",
            json!({
                "node_id": row.id, "hostname": row.hostname, "from": row.state, "to": transition.to,
                "event": change.event.kind, "deadline_at": transition.deadline,
            }),
        );
        app.wake();
    }
    Ok(Delivered {
        from: row.state,
        to: transition.to,
        duplicate,
    })
}

/// 把非 2xx 转换错误转为带节点上下文的说明（供日志）。
pub(crate) fn describe(failure: &Failure, node_id: i64) -> String {
    format!("节点 {node_id}：{failure}")
}
