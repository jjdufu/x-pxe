//! 事件入口 `POST /p/{token}/events`（JSON）与 `POST /p/{token}/events/{kind}`（安装器 `wget --post-data`）。
//!
//! 安装器事件要求引导令牌，agent 与模块事件要求 agent 令牌；`module.*` 只记进度不转换状态。

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path, State};
use serde_json::{Value, json};
use xpxe_domain::{EventEnvelope, EventId, EventSource, NodeEvent, SessionId};
use xpxe_store::{EventRecord, TokenKind};

use crate::app::App;
use crate::auth::{Actor, ClientIp};
use crate::transition::{Incoming, deliver};
use crate::{Failure, Outcome, internal};

/// 解析后的事件。
#[derive(Debug, Clone, PartialEq, Eq)]
struct Reported {
    /// 发送方事件 ID。
    event_id: String,
    /// 种类。
    kind: String,
    /// 附加数据。
    payload: Value,
}

/// 解析正文：JSON 或 `key=value` 空格分隔文本。
///
/// # Errors
///
/// 正文不合法时返回 400。
fn parse_body(body: &[u8], path_kind: Option<&str>) -> Outcome<Reported> {
    let text = std::str::from_utf8(body)
        .map_err(|_error| Failure::bad_request("BAD_BODY", "正文不是 UTF-8"))?;
    if text.trim_start().starts_with('{') {
        let value: Value = serde_json::from_str(text)
            .map_err(|error| Failure::bad_request("BAD_BODY", error.to_string()))?;
        let field = |name: &str| value.get(name).and_then(Value::as_str).map(str::to_owned);
        return Ok(Reported {
            event_id: field("event_id")
                .ok_or_else(|| Failure::bad_request("BAD_BODY", "缺少 event_id"))?,
            kind: field("kind")
                .or_else(|| path_kind.map(str::to_owned))
                .ok_or_else(|| Failure::bad_request("BAD_BODY", "缺少 kind"))?,
            payload: value.get("payload").cloned().unwrap_or(Value::Null),
        });
    }
    let pairs: BTreeMap<&str, &str> = text
        .split_whitespace()
        .filter_map(|pair| pair.split_once('='))
        .collect();
    let kind = pairs
        .get("event")
        .copied()
        .or(path_kind)
        .ok_or_else(|| Failure::bad_request("BAD_BODY", "缺少 event"))?;
    let event_id = pairs
        .get("event_id")
        .copied()
        .ok_or_else(|| Failure::bad_request("BAD_BODY", "缺少 event_id"))?;
    let payload: BTreeMap<&str, &str> = pairs
        .iter()
        .filter(|(key, _)| !matches!(**key, "event" | "event_id"))
        .map(|(key, value)| (*key, *value))
        .collect();
    Ok(Reported {
        event_id: event_id.to_owned(),
        kind: kind.to_owned(),
        payload: json!(payload),
    })
}

/// 种类 → 状态机事件与所需令牌；`None` 表示只记进度。
/// 事件路由：需要的令牌种类、对应的状态机事件（`None` 为纯进度）与来源。
type Route = (TokenKind, Option<NodeEvent>, EventSource);

///
/// # Errors
///
/// 不在白名单时返回 400。
fn classify(kind: &str) -> Outcome<Route> {
    Ok(match kind {
        "installer.started" => (
            TokenKind::Boot,
            Some(NodeEvent::InstallerStarted),
            EventSource::Installer,
        ),
        "installer.finished" => (
            TokenKind::Boot,
            Some(NodeEvent::InstallerFinished),
            EventSource::Installer,
        ),
        "installer.failed" => (
            TokenKind::Boot,
            Some(NodeEvent::InstallerFailed),
            EventSource::Installer,
        ),
        "agent.started" => (
            TokenKind::Agent,
            Some(NodeEvent::AgentStarted),
            EventSource::Agent,
        ),
        "agent.finished" => (
            TokenKind::Agent,
            Some(NodeEvent::AgentFinished),
            EventSource::Agent,
        ),
        "agent.failed" => (
            TokenKind::Agent,
            Some(NodeEvent::AgentFailed),
            EventSource::Agent,
        ),
        "module.started" | "module.finished" => (TokenKind::Agent, None, EventSource::Agent),
        other => {
            return Err(Failure::bad_request(
                "UNKNOWN_EVENT",
                format!("不支持的事件种类 {other}"),
            ));
        }
    })
}

/// 规范化事件 ID：安装器三类事件共用同一个渲染时 ID，按种类加后缀保证各自幂等。
///
/// # Errors
///
/// ID 不合法时返回 400。
fn event_id(reported: &Reported) -> Outcome<EventId> {
    let raw = if reported.event_id.contains(&reported.kind) {
        reported.event_id.clone()
    } else {
        format!("{}.{}", reported.event_id, reported.kind)
    };
    EventId::parse(&raw).map_err(|error| Failure::bad_request("BAD_EVENT_ID", error.to_string()))
}

/// 只记进度（`module.*`）：状态与截止时间不变。
///
/// # Errors
///
/// 写入失败时返回错误。
fn record_progress(
    app: &App,
    node_id: i64,
    session: &SessionId,
    id: EventId,
    reported: &Reported,
) -> Outcome<Value> {
    let row = app.store.nodes().get(node_id)?;
    if row.active_session_id.as_ref() != Some(session) {
        return Err(Failure::from(xpxe_domain::TransitionError::StaleSession));
    }
    let _inserted = app.store.nodes().record(
        node_id,
        &EventRecord {
            event_id: id,
            session_id: Some(session.clone()),
            at: app.now(),
            source: "agent".to_owned(),
            kind: reported.kind.clone(),
            from_state: row.state,
            to_state: row.state,
            deadline_at: row.deadline_at,
            active_session_id: row.active_session_id.clone(),
            payload: reported.payload.clone(),
        },
    )?;
    app.push(
        "node.event",
        json!({ "node_id": node_id, "kind": reported.kind, "payload": reported.payload }),
    );
    Ok(json!({ "state": row.state }))
}

/// 公共处理。
///
/// # Errors
///
/// 令牌被拒（403）、种类不在白名单（400）或转换被拒（409）时返回错误。
fn handle(app: &App, token: &str, ip: String, reported: &Reported) -> Outcome<Value> {
    let (session, kind) = app
        .store
        .tokens()
        .resolve(token, app.now())?
        .ok_or_else(|| Failure::forbidden("TOKEN_REJECTED", "令牌无效、已过期或会话已关闭"))?;
    let (needed, transition, source) = classify(&reported.kind)?;
    if kind != needed {
        return Err(Failure::forbidden("TOKEN_KIND", "该事件需要另一种令牌"));
    }
    let node_id = app.store.tokens().get(&session)?.node_id;
    let id = event_id(reported)?;
    let Some(event) = transition else {
        return record_progress(app, node_id, &session, id, reported);
    };
    let delivered = deliver(
        app,
        &Incoming {
            node_id,
            envelope: EventEnvelope {
                id,
                session: Some(session),
                source,
                event,
            },
            actor: Actor {
                name: format!("{source:?}").to_lowercase(),
                ip,
            },
            payload: reported.payload.clone(),
        },
    )?;
    Ok(json!({ "state": delivered.to, "duplicate": delivered.duplicate }))
}

/// `POST /p/{token}/events`。
///
/// # Errors
///
/// 见 [`handle`]。
pub(crate) async fn post_json(
    State(app): State<Arc<App>>,
    ClientIp(ip): ClientIp,
    Path(token): Path<String>,
    body: Bytes,
) -> Outcome<Json<Value>> {
    let reported = parse_body(&body, None)?;
    let worker = Arc::clone(&app);
    tokio::task::spawn_blocking(move || handle(&worker, &token, ip, &reported))
        .await
        .map_err(internal)?
        .map(Json)
}

/// `POST /p/{token}/events/{kind}`。
///
/// # Errors
///
/// 见 [`handle`]。
pub(crate) async fn post_kind(
    State(app): State<Arc<App>>,
    ClientIp(ip): ClientIp,
    Path((token, kind)): Path<(String, String)>,
    body: Bytes,
) -> Outcome<Json<Value>> {
    let reported = parse_body(&body, Some(&kind))?;
    let worker = Arc::clone(&app);
    tokio::task::spawn_blocking(move || handle(&worker, &token, ip, &reported))
        .await
        .map_err(internal)?
        .map(Json)
}

#[cfg(test)]
mod tests;
