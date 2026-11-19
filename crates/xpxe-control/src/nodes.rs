//! 节点：登记（含 BIOS + Redfish 拒绝）、列表、详情、事件与管理员状态操作。

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use xpxe_domain::init::TargetPlan;
use xpxe_domain::{
    BootChannel, ConfigDocument, ConfigError, ConfigKind, EventEnvelope, EventSource, FieldError,
    NodeEvent, NodeSpec, NodeSpecParts, Profile, migrate_current,
};
use xpxe_store::{Authorship, NewNode, NodeRow, Role, Store};

use crate::app::App;
use crate::auth::Principal;
use crate::configs::find_by_name;
use crate::transition::{Incoming, deliver, fresh_id};
use crate::{Failure, FieldIssue, Outcome};

/// 节点规格对象：规格本体 + 绑定的 Profile 与初始化计划。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NodeDoc {
    /// 规格。
    pub(crate) spec: NodeSpec,
    /// Profile 对象 ID。
    pub(crate) profile_id: i64,
    /// 初始化计划对象 ID。
    pub(crate) init_plan_id: i64,
}

impl ConfigDocument for NodeDoc {
    const KIND: ConfigKind = ConfigKind::NodeSpec;
    const SCHEMA: u32 = 1;

    fn validate(&self) -> Result<(), Vec<FieldError>> {
        self.spec.check_constraints()
    }

    fn migrate(from_schema: u32, value: Value) -> Result<Self, ConfigError> {
        migrate_current(from_schema, value)
    }
}

/// 读取节点与其规格。
///
/// # Errors
///
/// 不存在或读取失败时返回错误。
pub(crate) fn load(store: &Store, node_id: i64) -> Outcome<(NodeRow, NodeDoc)> {
    let row = store.nodes().get(node_id)?;
    let (_version, doc) = store.configs().head::<NodeDoc>(row.spec_object_id)?;
    Ok((row, doc))
}

/// 节点 JSON。
pub(crate) fn node_json(app: &App, row: &NodeRow, doc: &NodeDoc) -> Value {
    let alert = app.volatile().alerts.get(&row.id).cloned();
    json!({
        "id": row.id, "hostname": row.hostname, "mac": row.mac, "state": row.state,
        "state_since": row.state_since, "deadline_at": row.deadline_at,
        "active_session_id": row.active_session_id.as_ref().map(xpxe_domain::SessionId::as_str),
        "channel": doc.spec.channel(), "firmware": doc.spec.firmware(), "network": doc.spec.network(),
        "profile_id": doc.profile_id, "init_plan_id": doc.init_plan_id, "alert": alert,
        "last_event_id": row.last_event_id, "spec": doc.spec, "spec_object_id": row.spec_object_id,
    })
}

/// 登记请求。
#[derive(Debug, Deserialize)]
pub(crate) struct RegisterBody {
    /// 规格字段。
    pub(crate) spec: Value,
    /// Profile 对象 ID。
    pub(crate) profile_id: i64,
    /// 初始化计划对象 ID。
    pub(crate) init_plan_id: i64,
}

/// 校验登记请求，返回规格。
///
/// # Errors
///
/// 字段不合法、BIOS + Redfish、引用的 Profile/计划/BMC 不存在时返回 422。
pub(crate) fn check_register(app: &App, body: &RegisterBody) -> Outcome<NodeSpec> {
    let parts: NodeSpecParts = serde_json::from_value(body.spec.clone()).map_err(|error| {
        Failure::invalid(vec![FieldIssue::new(
            "/spec",
            "bad_spec",
            &error.to_string(),
        )])
    })?;
    let spec = NodeSpec::new(parts).map_err(|errors| {
        Failure::invalid(
            errors
                .iter()
                .map(|error| {
                    FieldIssue::new(&format!("/spec{}", error.path), &error.code, &error.message)
                })
                .collect(),
        )
    })?;
    let mut issues = Vec::new();
    if app
        .store
        .configs()
        .head::<Profile>(body.profile_id)
        .is_err()
    {
        issues.push(FieldIssue::new(
            "/profile_id",
            "unknown_profile",
            "Profile 不存在",
        ));
    }
    if app
        .store
        .configs()
        .head::<TargetPlan>(body.init_plan_id)
        .is_err()
    {
        issues.push(FieldIssue::new(
            "/init_plan_id",
            "unknown_init_plan",
            "初始化计划不存在",
        ));
    }
    if let BootChannel::Redfish { bmc } = spec.channel()
        && find_by_name(&app.store, ConfigKind::Bmc, bmc.as_str())?.is_none()
    {
        issues.push(FieldIssue::new(
            "/spec/channel/bmc",
            "unknown_bmc",
            "BMC 不存在",
        ));
    }
    if issues.is_empty() {
        Ok(spec)
    } else {
        Err(Failure::invalid(issues))
    }
}

/// `POST /nodes`。
///
/// # Errors
///
/// 角色不足、校验失败或 MAC/主机名冲突时返回错误。
pub(crate) async fn register(
    State(app): State<Arc<App>>,
    principal: Principal,
    Json(body): Json<RegisterBody>,
) -> Outcome<Json<Value>> {
    principal.require(Role::Operator)?;
    let spec = check_register(&app, &body)?;
    let now = app.now();
    let doc = NodeDoc {
        spec,
        profile_id: body.profile_id,
        init_plan_id: body.init_plan_id,
    };
    let author = Authorship {
        author_id: Some(principal.user.id),
        comment: "登记节点".to_owned(),
        at: now,
    };
    let version = app
        .store
        .configs()
        .create(doc.spec.hostname().as_str(), &doc, &author)?;
    let row = app.store.nodes().create(
        &NewNode {
            spec_object_id: version.object_id,
            mac: doc.spec.mac(),
            hostname: doc.spec.hostname().clone(),
        },
        now,
    )?;
    app.audit(&principal.actor().record(
        "node.create",
        &format!("node:{}", row.id),
        "ok",
        json!({ "hostname": row.hostname }),
    ));
    Ok(Json(node_json(&app, &row, &doc)))
}

/// `GET /nodes`。
///
/// # Errors
///
/// 读取失败时返回错误。
pub(crate) async fn list(
    State(app): State<Arc<App>>,
    _principal: Principal,
) -> Outcome<Json<Value>> {
    let mut out = Vec::new();
    for row in app.store.nodes().list()? {
        let (_version, doc) = app.store.configs().head::<NodeDoc>(row.spec_object_id)?;
        out.push(node_json(&app, &row, &doc));
    }
    Ok(Json(Value::Array(out)))
}

/// `GET /nodes/{id}`。
///
/// # Errors
///
/// 不存在或读取失败时返回错误。
pub(crate) async fn detail(
    State(app): State<Arc<App>>,
    _principal: Principal,
    Path(id): Path<i64>,
) -> Outcome<Json<Value>> {
    let (row, doc) = load(&app.store, id)?;
    Ok(Json(node_json(&app, &row, &doc)))
}

/// `GET /nodes/{id}/events`。
///
/// # Errors
///
/// 读取失败时返回错误。
pub(crate) async fn events(
    State(app): State<Arc<App>>,
    _principal: Principal,
    Path(id): Path<i64>,
) -> Outcome<Json<Value>> {
    Ok(Json(json!(app.store.nodes().events(id, 200)?)))
}

/// 投递一个管理员事件。
///
/// # Errors
///
/// 角色不足或转换被拒时返回错误。
pub(crate) fn admin_event(
    app: &App,
    principal: &Principal,
    node_id: i64,
    event: NodeEvent,
) -> Outcome<Value> {
    principal.require(Role::Operator)?;
    let label = event.kind().as_str();
    let delivered = deliver(
        app,
        &Incoming {
            node_id,
            envelope: EventEnvelope {
                id: fresh_id(app, &format!("admin.{label}"), node_id)?,
                session: None,
                source: EventSource::Admin,
                event,
            },
            actor: principal.actor(),
            payload: json!({ "user": principal.user.username }),
        },
    )?;
    Ok(json!({ "from": delivered.from, "to": delivered.to }))
}

/// `POST /nodes/{id}/disarm`。
///
/// # Errors
///
/// 角色不足或转换被拒时返回错误。
pub(crate) async fn disarm(
    State(app): State<Arc<App>>,
    principal: Principal,
    Path(id): Path<i64>,
) -> Outcome<Json<Value>> {
    admin_event(&app, &principal, id, NodeEvent::Disarm).map(Json)
}

/// `POST /nodes/{id}/reset`。
///
/// # Errors
///
/// 角色不足或转换被拒时返回错误。
pub(crate) async fn reset(
    State(app): State<Arc<App>>,
    principal: Principal,
    Path(id): Path<i64>,
) -> Outcome<Json<Value>> {
    admin_event(&app, &principal, id, NodeEvent::Reset).map(Json)
}
