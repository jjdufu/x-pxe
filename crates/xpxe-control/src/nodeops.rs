//! 节点详情抽屉用到的端点：保存规格新版本、事件时间轴（含附加数据）、会话详情（固定版本、事件、日志）。

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use serde::Deserialize;
use serde_json::{Value, json};
use xpxe_domain::{NodeState, SessionId};
use xpxe_store::{Authorship, Role};

use crate::app::App;
use crate::artifacts::{self, BOOT_TOKEN};
use crate::auth::Principal;
use crate::nodes::{NodeDoc, RegisterBody, check_register, load, node_json};
use crate::{Failure, FieldIssue, Outcome, internal};

/// 保存规格请求。
#[derive(Debug, Deserialize)]
pub(crate) struct UpdateBody {
    /// 客户端读取时的规格头版本。
    expected_head: u64,
    /// 规格字段。
    spec: Value,
    /// Profile 对象 ID。
    profile_id: i64,
    /// 初始化计划对象 ID。
    init_plan_id: i64,
    /// 变更说明。
    comment: Option<String>,
}

/// 可修改规格的状态（装机进行中不允许）。
const fn editable(state: NodeState) -> bool {
    matches!(
        state,
        NodeState::Idle | NodeState::Ready | NodeState::Failed
    )
}

/// `PUT /nodes/{id}`：保存 `node_spec` 新版本；主机名与 MAC 不可修改。
///
/// # Errors
///
/// 角色不足、装机进行中、修改主机名/MAC、校验失败或并发修改时返回错误。
pub(crate) async fn update(
    State(app): State<Arc<App>>,
    principal: Principal,
    Path(id): Path<i64>,
    Json(body): Json<UpdateBody>,
) -> Outcome<Json<Value>> {
    principal.require(Role::Operator)?;
    let (row, old) = load(&app.store, id)?;
    if !editable(row.state) {
        return Err(Failure::conflict(
            "NODE_BUSY",
            format!("节点处于 {}，装机结束后才能修改规格", row.state.as_str()),
        ));
    }
    let register = RegisterBody {
        spec: body.spec,
        profile_id: body.profile_id,
        init_plan_id: body.init_plan_id,
    };
    let spec = check_register(&app, &register)?;
    if spec.hostname() != old.spec.hostname() || spec.mac() != old.spec.mac() {
        return Err(Failure::invalid(vec![FieldIssue::new(
            "/spec/hostname",
            "immutable",
            "主机名与 MAC 登记后不可修改",
        )]));
    }
    let doc = NodeDoc {
        spec,
        profile_id: register.profile_id,
        init_plan_id: register.init_plan_id,
    };
    let author = Authorship {
        author_id: Some(principal.user.id),
        comment: body.comment.unwrap_or_else(|| "修改节点规格".to_owned()),
        at: app.now(),
    };
    let version =
        app.store
            .configs()
            .save(row.spec_object_id, &doc, body.expected_head, &author)?;
    app.audit(&principal.actor().record(
        "node.update",
        &format!("node:{id}"),
        "ok",
        json!(version),
    ));
    app.push(
        "config.version",
        json!({ "kind": "node_spec", "version": version }),
    );
    Ok(Json(
        json!({ "node": node_json(&app, &row, &doc), "version": version }),
    ))
}

/// `GET /nodes/{id}/spec`：规格头版本与文档（编辑表单用）。
///
/// # Errors
///
/// 不存在时返回错误。
pub(crate) async fn spec(
    State(app): State<Arc<App>>,
    _principal: Principal,
    Path(id): Path<i64>,
) -> Outcome<Json<Value>> {
    let row = app.store.nodes().get(id)?;
    let (version, doc) = app.store.configs().head::<NodeDoc>(row.spec_object_id)?;
    Ok(Json(json!({ "version": version, "document": doc })))
}

/// `GET /nodes/{id}/timeline`：最近 200 条事件（含附加数据）。
///
/// # Errors
///
/// 读取失败时返回错误。
pub(crate) async fn timeline(
    State(app): State<Arc<App>>,
    _principal: Principal,
    Path(id): Path<i64>,
) -> Outcome<Json<Value>> {
    Ok(Json(json!(app.store.nodes().details(Some(id), 200)?)))
}

/// 令牌末 4 位（会话关闭后令牌原文已删除，返回 `None`）。
///
/// # Errors
///
/// 读取会话目录失败时返回错误。
fn token_tail(app: &App, session: &str) -> Outcome<Option<String>> {
    Ok(
        artifacts::read_text(&app.settings, session, BOOT_TOKEN)?.map(|token| {
            let chars: Vec<char> = token.trim().chars().collect();
            chars
                .iter()
                .skip(chars.len().saturating_sub(4))
                .collect::<String>()
        }),
    )
}

/// 会话日志：d-i 失败上报（选盘清单等）与 agent 日志尾部，取自事件附加数据。
fn session_log(events: &[xpxe_store::EventDetail]) -> String {
    events
        .iter()
        .filter(|event| event.kind.ends_with(".failed") || event.payload.get("log_tail").is_some())
        .fold(String::new(), |mut log, event| {
            let body = event
                .payload
                .get("log_tail")
                .and_then(Value::as_str)
                .map_or_else(|| event.payload.to_string(), str::to_owned);
            for part in [
                "# ",
                event.kind.as_str(),
                " ",
                &event.at.unix_seconds().to_string(),
                "
",
                &body,
                "
",
            ] {
                log.push_str(part);
            }
            log
        })
}

/// `GET /nodes/{id}/sessions/{sid}`：固定的 Profile/计划/资产版本、令牌末 4 位、事件与日志。
///
/// # Errors
///
/// 会话不存在或不属于该节点时返回错误。
pub(crate) async fn session(
    State(app): State<Arc<App>>,
    _principal: Principal,
    Path((id, sid)): Path<(i64, String)>,
) -> Outcome<Json<Value>> {
    let session_id = SessionId::parse(&sid)
        .map_err(|_error| Failure::bad_request("BAD_SESSION", "会话 ID 不合法"))?;
    let record = app.store.tokens().get(&session_id)?;
    if record.node_id != id {
        return Err(Failure::not_found("会话不存在"));
    }
    let events = app.store.nodes().session_events(session_id.as_str())?;
    let pinned = serde_json::to_value(&record.pinned).map_err(internal)?;
    Ok(Json(json!({
        "id": record.id.as_str(), "channel": record.channel, "expires_at": record.expires_at,
        "closed_at": record.closed_at, "outcome": record.outcome,
        "preseed_served_at": record.preseed_served_at, "plan_served_at": record.plan_served_at,
        "pinned": pinned, "token_tail": token_tail(&app, session_id.as_str())?,
        "log": session_log(&events), "events": events,
    })))
}
