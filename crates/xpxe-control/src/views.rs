//! 管理界面的浏览端点：用户与会话、API Token 列表、审计分页与导出、配置历史、总览统计、
//! 预设与初始化模块目录。

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::response::{IntoResponse as _, Response};
use http::{StatusCode, header};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use xpxe_domain::init::{
    ModuleId, ModuleSet, PackageCatalog, PackageGroup, SysctlParams, SysctlPreset, TargetPlan,
};
use xpxe_domain::{ConfigKind, NodeState, Profile, Timestamp};
use xpxe_store::{AuditQuery, Role, SESSION_KEY_LEN, UserPatch};

use crate::app::App;
use crate::auth::Principal;
use crate::{Failure, Outcome, internal};

/// Profile 预设。
const PROFILE_PRESETS: [&str; 2] = ["nexus-lvm", "official-mirror"];
/// 初始化计划预设。
const PLAN_PRESETS: [&str; 2] = ["deb13-init-scripts", "minimal"];
/// 配置历史覆盖的种类。
const HISTORY_KINDS: [ConfigKind; 7] = [
    ConfigKind::Profile,
    ConfigKind::InitPlan,
    ConfigKind::BootNetwork,
    ConfigKind::DhcpRules,
    ConfigKind::Timeouts,
    ConfigKind::NodeSpec,
    ConfigKind::BootMenu,
];
/// 配置历史最多返回条数。
const HISTORY_LIMIT: usize = 500;

/// `GET /users`（仅 admin）。
///
/// # Errors
///
/// 角色不足或读取失败时返回错误。
pub(crate) async fn users(
    State(app): State<Arc<App>>,
    principal: Principal,
) -> Outcome<Json<Value>> {
    principal.require(Role::Admin)?;
    Ok(Json(json!(app.store.accounts().users()?)))
}

/// 修改用户请求。
#[derive(Debug, Deserialize)]
pub(crate) struct PatchBody {
    /// 新角色。
    role: Option<String>,
    /// 是否禁用。
    disabled: Option<bool>,
}

/// `PATCH /users/{id}`（仅 admin）：不允许修改自己的角色或禁用自己。
///
/// # Errors
///
/// 角色不足、修改自己、角色未知或用户不存在时返回错误。
pub(crate) async fn patch_user(
    State(app): State<Arc<App>>,
    principal: Principal,
    Path(id): Path<i64>,
    Json(body): Json<PatchBody>,
) -> Outcome<Json<Value>> {
    principal.require(Role::Admin)?;
    if id == principal.user.id {
        return Err(Failure::bad_request(
            "SELF_LOCKOUT",
            "不能修改自己的角色或禁用自己",
        ));
    }
    let patch = UserPatch {
        role: body.role.as_deref().map(str::parse::<Role>).transpose()?,
        disabled: body.disabled,
    };
    let user = app.store.accounts().patch_user(id, patch)?;
    app.audit(&principal.actor().record(
        "user.update",
        &format!("user:{id}"),
        "ok",
        json!({ "role": user.role, "disabled": user.disabled }),
    ));
    Ok(Json(json!(user)))
}

/// 会话标识：会话 ID 哈希的前缀。
fn session_key(raw: &str) -> String {
    hex::encode(Sha256::digest(raw.as_bytes()))
        .chars()
        .take(SESSION_KEY_LEN)
        .collect()
}

/// `GET /me/sessions`：标出当前会话。
///
/// # Errors
///
/// 读取失败时返回错误。
pub(crate) async fn my_sessions(
    State(app): State<Arc<App>>,
    principal: Principal,
) -> Outcome<Json<Value>> {
    let current = principal.session.as_deref().map(session_key);
    let list: Vec<Value> = app
        .store
        .accounts()
        .sessions(principal.user.id)?
        .into_iter()
        .map(|item| {
            let is_current = current.as_deref() == Some(item.key.as_str());
            let mut value = json!(item);
            if let Some(object) = value.as_object_mut() {
                let _old = object.insert("current".to_owned(), json!(is_current));
            }
            value
        })
        .collect();
    Ok(Json(json!(list)))
}

/// `DELETE /me/sessions/{key}`。
///
/// # Errors
///
/// 会话不存在（404）或标识不合法（400）时返回错误。
pub(crate) async fn revoke_session(
    State(app): State<Arc<App>>,
    principal: Principal,
    Path(key): Path<String>,
) -> Outcome<StatusCode> {
    if !app
        .store
        .accounts()
        .revoke_session(principal.user.id, &key)?
    {
        return Err(Failure::not_found("会话不存在"));
    }
    app.audit(&principal.actor().record(
        "auth.session_revoke",
        &format!("session:{key}"),
        "ok",
        json!({}),
    ));
    Ok(StatusCode::NO_CONTENT)
}

/// `GET /me/tokens`。
///
/// # Errors
///
/// 读取失败时返回错误。
pub(crate) async fn my_tokens(
    State(app): State<Arc<App>>,
    principal: Principal,
) -> Outcome<Json<Value>> {
    Ok(Json(json!(app.store.accounts().tokens(principal.user.id)?)))
}

/// 审计查询参数。
#[derive(Debug, Default, Deserialize)]
pub(crate) struct AuditParams {
    /// 操作者。
    actor: Option<String>,
    /// 动作。
    action: Option<String>,
    /// 结果。
    outcome: Option<String>,
    /// 起始（Unix 秒）。
    since: Option<u64>,
    /// 截止（Unix 秒）。
    until: Option<u64>,
    /// 页码（从 1 开始）。
    page: Option<u32>,
    /// 每页条数。
    page_size: Option<u32>,
}

/// 空串视为不限。
fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|text| !text.trim().is_empty())
}

/// `GET /audit`（仅 admin）：分页与筛选。
///
/// # Errors
///
/// 角色不足或读取失败时返回错误。
pub(crate) async fn audit_list(
    State(app): State<Arc<App>>,
    principal: Principal,
    Query(params): Query<AuditParams>,
) -> Outcome<Json<Value>> {
    principal.require(Role::Admin)?;
    let limit = params
        .page_size
        .unwrap_or(25)
        .clamp(1, xpxe_store::AUDIT_PAGE_MAX);
    let page = params.page.unwrap_or(1).max(1);
    let query = AuditQuery {
        actor: non_empty(params.actor),
        action: non_empty(params.action),
        outcome: non_empty(params.outcome),
        since: params.since.map(Timestamp::from_unix_seconds),
        until: params.until.map(Timestamp::from_unix_seconds),
        offset: page.saturating_sub(1).saturating_mul(limit),
        limit,
    };
    Ok(Json(json!(app.store.audit().page(&query)?)))
}

/// 导出参数。
#[derive(Debug, Deserialize)]
pub(crate) struct ExportParams {
    /// 起始（Unix 秒，缺省为全部）。
    since: Option<u64>,
}

/// `GET /audit/export`（仅 admin）：JSON Lines。
///
/// # Errors
///
/// 角色不足或导出失败时返回错误。
pub(crate) async fn audit_export(
    State(app): State<Arc<App>>,
    principal: Principal,
    Query(params): Query<ExportParams>,
) -> Outcome<Response> {
    principal.require(Role::Admin)?;
    let mut out = Vec::new();
    let count = app.store.audit().export_since(
        Timestamp::from_unix_seconds(params.since.unwrap_or(0)),
        &mut out,
    )?;
    app.audit(
        &principal
            .actor()
            .record("audit.export", "audit", "ok", json!({ "count": count })),
    );
    Ok((
        [
            (header::CONTENT_TYPE, "application/x-ndjson; charset=utf-8"),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"xpxe-audit.jsonl\"",
            ),
        ],
        out,
    )
        .into_response())
}

/// 用户 ID → 用户名。
///
/// # Errors
///
/// 读取失败时返回错误。
fn usernames(app: &App) -> Outcome<BTreeMap<i64, String>> {
    Ok(app
        .store
        .accounts()
        .users()?
        .into_iter()
        .map(|user| (user.id, user.username))
        .collect())
}

/// 历史查询参数。
#[derive(Debug, Deserialize)]
pub(crate) struct HistoryParams {
    /// 只看某种类。
    kind: Option<String>,
}

/// `GET /history`：全部配置对象的版本（从新到旧）。
///
/// # Errors
///
/// 读取失败时返回错误。
pub(crate) async fn history(
    State(app): State<Arc<App>>,
    _principal: Principal,
    Query(params): Query<HistoryParams>,
) -> Outcome<Json<Value>> {
    let names = usernames(&app)?;
    let mut rows: Vec<(u64, Value)> = Vec::new();
    let wanted = non_empty(params.kind);
    for kind in HISTORY_KINDS {
        if wanted.as_deref().is_some_and(|name| name != kind.as_str()) {
            continue;
        }
        for object in app.store.configs().list(kind)? {
            for version in app.store.configs().versions(object.id)? {
                let author = version.author_id.and_then(|id| names.get(&id).cloned());
                rows.push((
                    version.created_at.unix_seconds(),
                    json!({
                        "kind": kind.as_str(), "object_id": object.id, "name": object.name,
                        "head_version": object.head_version, "version": version.version,
                        "parent_version": version.parent_version, "author": author,
                        "created_at": version.created_at, "comment": version.comment,
                        "content_sha256": version.content_sha256,
                    }),
                ));
            }
        }
    }
    rows.sort_by_key(|row| std::cmp::Reverse(row.0));
    rows.truncate(HISTORY_LIMIT);
    Ok(Json(json!(
        rows.into_iter().map(|(_, row)| row).collect::<Vec<_>>()
    )))
}

/// 状态 → 计数。
///
/// # Errors
///
/// 读取失败时返回错误。
fn state_counts(app: &App) -> Outcome<Value> {
    let mut counts: BTreeMap<&'static str, u64> = BTreeMap::new();
    for row in app.store.nodes().list()? {
        let entry = counts.entry(row.state.as_str()).or_insert(0);
        *entry = entry.saturating_add(1);
    }
    let idle = counts.get(NodeState::Idle.as_str()).copied().unwrap_or(0);
    Ok(json!({ "by_state": counts, "idle": idle }))
}

/// `GET /overview`：节点状态计数、近 30 天装机结果、最近 20 条节点事件。
///
/// # Errors
///
/// 读取失败时返回错误。
pub(crate) async fn overview(
    State(app): State<Arc<App>>,
    _principal: Principal,
) -> Outcome<Json<Value>> {
    let now = app.now();
    let since = Timestamp::from_unix_seconds(
        now.unix_seconds()
            .saturating_sub(Duration::from_hours(30 * 24).as_secs()),
    );
    Ok(Json(json!({
        "now": now,
        "states": state_counts(&app)?,
        "outcomes": app.store.nodes().outcomes(since)?,
        "events": app.store.nodes().details(None, 20)?,
    })))
}

/// `GET /presets`：内置 Profile 与初始化计划预设（补全默认值后的完整文档）。
///
/// # Errors
///
/// 预设加载失败时返回错误。
pub(crate) async fn presets(_principal: Principal) -> Outcome<Json<Value>> {
    let mut profiles = serde_json::Map::new();
    for name in PROFILE_PRESETS {
        let doc = Profile::load_preset(name)?;
        let _old = profiles.insert(
            name.to_owned(),
            serde_json::to_value(doc).map_err(internal)?,
        );
    }
    let mut plans = serde_json::Map::new();
    for name in PLAN_PRESETS {
        let doc = TargetPlan::load_preset(name)?;
        let _old = plans.insert(
            name.to_owned(),
            serde_json::to_value(doc).map_err(internal)?,
        );
    }
    Ok(Json(json!({ "profile": profiles, "init_plan": plans })))
}

/// 包分组目录。
///
/// # Errors
///
/// 内置目录无法解析时返回错误。
fn package_groups() -> Outcome<Value> {
    let catalog = PackageCatalog::builtin()?;
    let groups: Vec<Value> = PackageGroup::ALL
        .into_iter()
        .map(|group| {
            let packages = catalog.group(group);
            json!({ "id": group, "count": packages.len(), "packages": packages })
        })
        .collect();
    Ok(json!(groups))
}

/// `GET /init-modules`：20 个模块（执行顺序、标题、默认超时、默认设置）、包分组、sysctl 预设。
///
/// # Errors
///
/// 内置目录无法解析时返回错误。
pub(crate) async fn init_modules(_principal: Principal) -> Outcome<Json<Value>> {
    let defaults = serde_json::to_value(ModuleSet::default()).map_err(internal)?;
    let modules: Vec<Value> = ModuleId::ALL
        .into_iter()
        .enumerate()
        .map(|(index, id)| {
            json!({
                "order": index.saturating_add(1), "id": id.as_str(), "title": id.title(),
                "default_timeout_s": id.default_timeout_s(),
                "defaults": defaults.get(id.as_str()).cloned().unwrap_or(Value::Null),
            })
        })
        .collect();
    let mut sysctl = serde_json::Map::new();
    for preset in [
        SysctlPreset::Deb13InitScripts,
        SysctlPreset::WebServer,
        SysctlPreset::Minimal,
    ] {
        let name = serde_json::to_value(preset).map_err(internal)?;
        let params = serde_json::to_value(SysctlParams::for_preset(preset)).map_err(internal)?;
        let _old = sysctl.insert(name.as_str().unwrap_or_default().to_owned(), params);
    }
    Ok(Json(json!({
        "modules": modules,
        "package_groups": package_groups()?,
        "sysctl_presets": sysctl,
    })))
}
