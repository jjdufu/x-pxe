//! 配置对象：列表、创建、读取、保存新版本（乐观并发）、版本列表与回滚。

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use xpxe_domain::init::TargetPlan;
use xpxe_domain::{BootNetwork, ConfigDocument, ConfigKind, DhcpRules, Profile, Timeouts};
use xpxe_store::{Authorship, Role, Store, VersionRef};

use crate::app::App;
use crate::auth::Principal;
use crate::bmc::BmcDoc;
use crate::nodes::NodeDoc;
use crate::{Failure, FieldIssue, Outcome};

/// 解析并校验文档。
///
/// # Errors
///
/// 结构不合法或校验失败时返回 422。
pub(crate) fn parse_doc<T: ConfigDocument + DeserializeOwned>(document: Value) -> Outcome<T> {
    let doc: T = serde_json::from_value(document).map_err(|error| {
        Failure::invalid(vec![FieldIssue::new(
            "/document",
            "bad_document",
            &error.to_string(),
        )])
    })?;
    doc.validate()
        .map_err(|errors| Failure::invalid(errors.iter().map(FieldIssue::from).collect()))?;
    Ok(doc)
}

/// 某种类的“默认”对象（名为 `default`，否则第一个）的头版本。
///
/// # Errors
///
/// 读取失败时返回错误。
pub(crate) fn singleton<T: ConfigDocument>(store: &Store) -> Outcome<Option<(VersionRef, T)>> {
    let list = store.configs().list(T::KIND)?;
    let chosen = list
        .iter()
        .find(|info| info.name == "default")
        .or_else(|| list.first());
    match chosen {
        Some(info) => Ok(Some(store.configs().head::<T>(info.id)?)),
        None => Ok(None),
    }
}

/// 按名称查找对象 ID。
///
/// # Errors
///
/// 读取失败时返回错误。
pub(crate) fn find_by_name(store: &Store, kind: ConfigKind, name: &str) -> Outcome<Option<i64>> {
    Ok(store
        .configs()
        .list(kind)?
        .into_iter()
        .find(|info| info.name == name)
        .map(|info| info.id))
}

/// 作者信息。
fn authorship(app: &App, principal: &Principal, comment: Option<String>) -> Authorship {
    Authorship {
        author_id: Some(principal.user.id),
        comment: comment.unwrap_or_default(),
        at: app.now(),
    }
}

/// 可写种类。
const WRITABLE: [&str; 5] = [
    "profile",
    "init_plan",
    "boot_network",
    "dhcp_rules",
    "timeouts",
];

/// 按种类名分派到泛型函数。
macro_rules! by_kind {
    ($kind:expr, $func:ident, $($arg:expr),*) => {
        match $kind {
            "profile" => $func::<Profile>($($arg),*),
            "init_plan" => $func::<TargetPlan>($($arg),*),
            "boot_network" => $func::<BootNetwork>($($arg),*),
            "dhcp_rules" => $func::<DhcpRules>($($arg),*),
            "timeouts" => $func::<Timeouts>($($arg),*),
            "node_spec" => $func::<NodeDoc>($($arg),*),
            "bmc" => $func::<BmcDoc>($($arg),*),
            other => Err(Failure::not_found(format!("未知配置种类 {other}"))),
        }
    };
}

/// 要求可写种类。
///
/// # Errors
///
/// 节点规格与 BMC 须走各自端点，返回 400。
fn writable(kind: &str) -> Outcome<()> {
    if WRITABLE.contains(&kind) {
        Ok(())
    } else {
        Err(Failure::bad_request(
            "READ_ONLY_KIND",
            format!("{kind} 请使用专用端点修改"),
        ))
    }
}

/// 种类枚举。
///
/// # Errors
///
/// 失败时返回错误。
fn list_of<T: ConfigDocument>(app: &App) -> Outcome<Value> {
    Ok(json!(app.store.configs().list(T::KIND)?))
}

/// `GET /configs/{kind}`。
///
/// # Errors
///
/// 种类未知或读取失败时返回错误。
pub(crate) async fn list(
    State(app): State<Arc<App>>,
    _principal: Principal,
    Path(kind): Path<String>,
) -> Outcome<Json<Value>> {
    by_kind!(kind.as_str(), list_of, &app).map(Json)
}

/// 新建请求。
#[derive(Debug, Deserialize)]
pub(crate) struct CreateBody {
    /// 对象名。
    name: String,
    /// 文档。
    document: Value,
    /// 说明。
    comment: Option<String>,
}

/// 新建对象。
///
/// # Errors
///
/// 失败时返回错误。
fn create_of<T: ConfigDocument>(
    app: &App,
    body: CreateBody,
    author: &Authorship,
) -> Outcome<Value> {
    let doc: T = parse_doc(body.document)?;
    let version = app.store.configs().create(&body.name, &doc, author)?;
    Ok(json!(version))
}

/// `POST /configs/{kind}`。
///
/// # Errors
///
/// 角色不足、种类只读、文档不合法或名称冲突时返回错误。
pub(crate) async fn create(
    State(app): State<Arc<App>>,
    principal: Principal,
    Path(kind): Path<String>,
    Json(body): Json<CreateBody>,
) -> Outcome<Json<Value>> {
    principal.require(Role::Operator)?;
    writable(&kind)?;
    let author = authorship(&app, &principal, body.comment.clone());
    let name = body.name.clone();
    let version = by_kind!(kind.as_str(), create_of, &app, body, &author)?;
    app.audit(&principal.actor().record(
        "config.create",
        &format!("{kind}:{name}"),
        "ok",
        version.clone(),
    ));
    app.push(
        "config.version",
        json!({ "kind": kind, "version": version }),
    );
    Ok(Json(version))
}

/// 读取头版本。
///
/// # Errors
///
/// 失败时返回错误。
fn get_of<T: ConfigDocument>(app: &App, id: i64) -> Outcome<Value> {
    let (version, doc) = app.store.configs().head::<T>(id)?;
    Ok(json!({ "version": version, "document": doc }))
}

/// `GET /configs/{kind}/{id}`。
///
/// # Errors
///
/// 不存在或读取失败时返回错误。
pub(crate) async fn get(
    State(app): State<Arc<App>>,
    _principal: Principal,
    Path((kind, id)): Path<(String, i64)>,
) -> Outcome<Json<Value>> {
    by_kind!(kind.as_str(), get_of, &app, id).map(Json)
}

/// 保存请求。
#[derive(Debug, Deserialize)]
pub(crate) struct SaveBody {
    /// 客户端读取时的头版本。
    expected_head: u64,
    /// 文档。
    document: Value,
    /// 说明。
    comment: Option<String>,
}

/// 保存新版本。
///
/// # Errors
///
/// 失败时返回错误。
fn save_of<T: ConfigDocument>(
    app: &App,
    id: i64,
    body: SaveBody,
    author: &Authorship,
) -> Outcome<Value> {
    let doc: T = parse_doc(body.document)?;
    Ok(json!(app.store.configs().save(
        id,
        &doc,
        body.expected_head,
        author
    )?))
}

/// `PUT /configs/{kind}/{id}`：头版本不符时 409。
///
/// # Errors
///
/// 角色不足、并发修改、文档不合法时返回错误。
pub(crate) async fn save(
    State(app): State<Arc<App>>,
    principal: Principal,
    Path((kind, id)): Path<(String, i64)>,
    Json(body): Json<SaveBody>,
) -> Outcome<Json<Value>> {
    principal.require(Role::Operator)?;
    writable(&kind)?;
    let author = authorship(&app, &principal, body.comment.clone());
    let version = by_kind!(kind.as_str(), save_of, &app, id, body, &author)?;
    app.audit(&principal.actor().record(
        "config.save",
        &format!("{kind}:{id}"),
        "ok",
        version.clone(),
    ));
    app.push(
        "config.version",
        json!({ "kind": kind, "version": version }),
    );
    Ok(Json(version))
}

/// `GET /configs/{kind}/{id}/versions`。
///
/// # Errors
///
/// 读取失败时返回错误。
pub(crate) async fn versions(
    State(app): State<Arc<App>>,
    _principal: Principal,
    Path((_kind, id)): Path<(String, i64)>,
) -> Outcome<Json<Value>> {
    Ok(Json(json!(app.store.configs().versions(id)?)))
}

/// 回滚请求。
#[derive(Debug, Deserialize)]
pub(crate) struct RollbackBody {
    /// 回滚到的版本。
    to_version: u64,
    /// 客户端读取时的头版本。
    expected_head: u64,
    /// 说明。
    comment: Option<String>,
}

/// 回滚（生成新版本）。
///
/// # Errors
///
/// 失败时返回错误。
fn rollback_of<T: ConfigDocument>(
    app: &App,
    id: i64,
    body: &RollbackBody,
    author: &Authorship,
) -> Outcome<Value> {
    Ok(json!(app.store.configs().rollback::<T>(
        id,
        body.to_version,
        body.expected_head,
        author
    )?))
}

/// `POST /configs/{kind}/{id}/rollback`。
///
/// # Errors
///
/// 角色不足、并发修改或目标版本不存在时返回错误。
pub(crate) async fn rollback(
    State(app): State<Arc<App>>,
    principal: Principal,
    Path((kind, id)): Path<(String, i64)>,
    Json(body): Json<RollbackBody>,
) -> Outcome<Json<Value>> {
    principal.require(Role::Operator)?;
    writable(&kind)?;
    let author = authorship(&app, &principal, body.comment.clone());
    let version = by_kind!(kind.as_str(), rollback_of, &app, id, &body, &author)?;
    app.audit(&principal.actor().record(
        "config.rollback",
        &format!("{kind}:{id}"),
        "ok",
        version.clone(),
    ));
    app.push(
        "config.version",
        json!({ "kind": kind, "version": version }),
    );
    Ok(Json(version))
}
