//! 草稿预览、校验与版本差异：不落库地渲染 Profile 与初始化计划，供编辑页实时预览；
//! 以及“复用现有 DHCP”模式下的现网 DHCP 配置片段。

use std::net::Ipv4Addr;
use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use serde::Deserialize;
use serde_json::{Value, json};
use xpxe_domain::init::{PackageCatalog, PlainPath, TargetPlan};
use xpxe_domain::{Change, ConfigDocument, FieldError, NodeSpec, Profile};
use xpxe_render::init::{DEFAULT_LOG_PATH, diff_lines, render_module, render_plan};
use xpxe_render::{
    ArmedState, RenderRequest, RenderRequestParts, RequestDigests, RequestTokens, render,
};

use crate::app::App;
use crate::auth::Principal;
use crate::configs::parse_doc;
use crate::nodes::load;
use crate::{Failure, FieldIssue, Outcome, internal};

/// 预览用的占位令牌（只出现在预览文本中，任何会话都不会签发它）。
const PREVIEW_TOKEN: &str = "PREVIEW-boot-token";
/// 预览用的 agent 占位令牌。
const PREVIEW_AGENT: &str = "PREVIEW-agent-token";
/// 预览用的占位摘要。
const ZERO_SHA: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// 预览请求。
#[derive(Debug, Deserialize)]
pub(crate) struct PreviewBody {
    /// 草稿文档。
    document: Value,
    /// 预览上下文：`dhcp`、`static` 或 `node:<id>`。
    context: Option<String>,
    /// 初始化计划预览所用的 Profile 对象 ID（缺省为第一个 Profile，没有则用 `nexus-lvm` 预设）。
    profile_id: Option<i64>,
    /// 对比基准（对象 ID 与版本）。
    base: Option<BaseRef>,
}

/// 对比基准。
#[derive(Debug, Clone, Copy, Deserialize)]
pub(crate) struct BaseRef {
    /// 对象 ID。
    object_id: i64,
    /// 版本。
    version: u64,
}

/// 字段错误 → 接口格式。
fn issues(errors: &[FieldError]) -> Vec<FieldIssue> {
    errors.iter().map(FieldIssue::from).collect()
}

/// 解析失败统一为字段错误列表（不返回 422，预览需要展示错误）。
///
/// # Errors
///
/// 文档不合法时返回字段错误。
fn parse_or_issues<T: ConfigDocument + serde::de::DeserializeOwned>(
    document: Value,
) -> Result<T, Vec<FieldIssue>> {
    parse_doc::<T>(document).map_err(|failure| {
        if failure.fields().is_empty() {
            vec![FieldIssue::new(
                "/document",
                failure.code(),
                failure.message(),
            )]
        } else {
            failure.fields().to_vec()
        }
    })
}

/// 样例节点规格。
///
/// # Errors
///
/// `node:<id>` 指向的节点不存在时返回错误。
fn sample_spec(context: &str) -> Outcome<NodeSpec> {
    let network = if context == "static" {
        json!({ "type": "static", "address": "10.77.0.16/24", "gateway": "10.77.0.1", "dns": ["10.77.0.1"] })
    } else {
        json!({ "type": "dhcp" })
    };
    let spec = json!({
        "hostname": format!("sample-{context}"), "fqdn": null, "mac": "52:54:00:12:34:56",
        "os_family": "debian13", "firmware": "uefi", "channel": { "type": "lan" }, "network": network,
    });
    serde_json::from_value(spec).map_err(internal)
}

/// 预览上下文 → 节点规格。
///
/// # Errors
///
/// 节点不存在时返回错误。
fn context_spec(app: &App, context: Option<&str>) -> Outcome<NodeSpec> {
    match context.and_then(|raw| raw.strip_prefix("node:")) {
        Some(id) => {
            let node_id = id
                .parse::<i64>()
                .map_err(|_error| Failure::bad_request("BAD_CONTEXT", "节点 ID 不合法"))?;
            Ok(load(&app.store, node_id)?.1.spec)
        }
        None => sample_spec(context.unwrap_or("dhcp")),
    }
}

/// 渲染 Profile 的全部预览产物。
///
/// # Errors
///
/// 渲染失败时返回错误。
fn render_profile(app: &App, profile: &Profile, spec: NodeSpec) -> Outcome<Vec<(String, String)>> {
    let bad = |error: xpxe_domain::ParseError| internal(error);
    let request = RenderRequest::from_parts(RenderRequestParts {
        profile: profile.clone(),
        node: spec,
        boot_base: app.settings.boot_base.clone(),
        tokens: RequestTokens {
            boot_token: PREVIEW_TOKEN.parse().map_err(bad)?,
            agent_token: PREVIEW_AGENT.parse().map_err(bad)?,
            session_short: "PREVIEW0".to_owned(),
            event_id: "preview.installer".parse().map_err(bad)?,
        },
        digests: RequestDigests {
            agent_sha256: ZERO_SHA.to_owned(),
            plan_sha256: ZERO_SHA.to_owned(),
        },
        armed: ArmedState::armed(),
    });
    let rendered = render(&request)
        .map_err(|error| Failure::bad_request("RENDER_FAILED", error.to_string()))?;
    Ok(vec![
        ("boot.ipxe".to_owned(), rendered.boot_ipxe().to_owned()),
        ("cmdline".to_owned(), rendered.cmdline().to_owned()),
        ("preseed.cfg".to_owned(), rendered.preseed().to_owned()),
        (
            "disk-select.sh".to_owned(),
            rendered.disk_select().to_owned(),
        ),
    ])
}

/// 产物列表 → JSON。
fn artifacts_json(items: &[(String, String)]) -> Value {
    json!(
        items
            .iter()
            .map(|(name, text)| json!({ "name": name, "text": text }))
            .collect::<Vec<_>>()
    )
}

/// 两组产物的逐行差异（只列出有变化的）。
fn rendered_diff(old: &[(String, String)], new: &[(String, String)]) -> Value {
    json!(
        new.iter()
            .filter_map(|(name, text)| {
                let before = old
                    .iter()
                    .find(|(other, _)| other == name)
                    .map_or("", |(_, body)| body.as_str());
                (before != text).then(|| json!({ "name": name, "lines": diff_lines(before, text) }))
            })
            .collect::<Vec<_>>()
    )
}

/// 字段差异 → JSON。
fn changes_json(changes: &[Change]) -> Value {
    json!(changes
        .iter()
        .map(|change| json!({
            "path": change.path, "before": change.before.to_string(), "after": change.after.to_string(),
        }))
        .collect::<Vec<_>>())
}

/// 两个文档的字段差异。
///
/// # Errors
///
/// 规范化失败时返回错误。
fn doc_changes<T: ConfigDocument>(old: &T, new: &T) -> Outcome<Value> {
    let left = xpxe_domain::canonical_json(old)?;
    let right = xpxe_domain::canonical_json(new)?;
    Ok(changes_json(&xpxe_domain::diff(&left, &right)))
}

/// Profile 预览。
///
/// # Errors
///
/// 上下文或基准不存在时返回错误。
fn profile_preview(app: &App, body: PreviewBody) -> Outcome<Value> {
    let profile = match parse_or_issues::<Profile>(body.document) {
        Ok(profile) => profile,
        Err(fields) => return Ok(json!({ "ok": false, "fields": fields, "artifacts": [] })),
    };
    let spec = context_spec(app, body.context.as_deref())?;
    let artifacts = match render_profile(app, &profile, spec.clone()) {
        Ok(items) => items,
        Err(failure) => {
            let fields = vec![FieldIssue::new(
                "/document",
                failure.code(),
                failure.message(),
            )];
            return Ok(json!({ "ok": false, "fields": fields, "artifacts": [] }));
        }
    };
    let mut reply = json!({
        "ok": true, "fields": [], "artifacts": artifacts_json(&artifacts),
        "warnings": issues(&profile.offline_warnings(app.settings.offline)),
    });
    if let Some(base) = body.base {
        let old: Profile = app.store.configs().load(base.object_id, base.version)?;
        let old_artifacts = render_profile(app, &old, spec)?;
        if let Some(object) = reply.as_object_mut() {
            let _changes = object.insert("changes".to_owned(), doc_changes(&old, &profile)?);
            let _rendered = object.insert(
                "rendered_diff".to_owned(),
                rendered_diff(&old_artifacts, &artifacts),
            );
        }
    }
    Ok(reply)
}

/// 初始化计划预览所用的 Profile。
///
/// # Errors
///
/// 指定的 Profile 不存在时返回错误。
fn plan_profile(app: &App, profile_id: Option<i64>) -> Outcome<Profile> {
    if let Some(id) = profile_id {
        return Ok(app.store.configs().head::<Profile>(id)?.1);
    }
    let first = app.store.configs().list(xpxe_domain::ConfigKind::Profile)?;
    match first.first() {
        Some(info) => Ok(app.store.configs().head::<Profile>(info.id)?.1),
        None => Ok(Profile::load_preset("nexus-lvm")?),
    }
}

/// 逐模块渲染（不做跨模块校验，供出错时仍能预览）。
///
/// # Errors
///
/// 渲染失败时返回错误。
fn plan_modules(plan: &TargetPlan, profile: &Profile) -> Outcome<Vec<(String, String)>> {
    let log = PlainPath::parse(DEFAULT_LOG_PATH).map_err(internal)?;
    xpxe_domain::init::ModuleId::ALL
        .into_iter()
        .map(|id| {
            render_module(plan, profile, id, &log)
                .map(|script| (id.as_str().to_owned(), script))
                .map_err(|error| Failure::bad_request("RENDER_FAILED", error.to_string()))
        })
        .collect()
}

/// 整份计划文件（校验通过时）。
fn plan_file(app: &App, plan: &TargetPlan, profile: &Profile) -> Option<String> {
    let ctx = xpxe_render::init::PlanContext {
        session: "preview".parse().ok()?,
        events_url: xpxe_domain::HttpUrl::parse(&format!(
            "{}/p/{PREVIEW_AGENT}/events",
            app.settings.boot_base.as_str()
        ))
        .ok()?,
        log_path: PlainPath::parse(DEFAULT_LOG_PATH).ok()?,
        final_retry_s: 3600,
    };
    render_plan(plan, profile, &ctx)
        .ok()
        .map(|rendered| rendered.json)
}

/// 初始化计划预览。
///
/// # Errors
///
/// Profile 或基准不存在时返回错误。
fn plan_preview(app: &App, body: PreviewBody) -> Outcome<Value> {
    let plan = match parse_or_issues::<TargetPlan>(body.document) {
        Ok(plan) => plan,
        Err(fields) => return Ok(json!({ "ok": false, "fields": fields, "modules": [] })),
    };
    let profile = plan_profile(app, body.profile_id)?;
    let cross = plan
        .check_with(profile.accounts().primary_user.as_ref())
        .err()
        .map_or_else(Vec::new, |errors| issues(&errors));
    let modules = plan_modules(&plan, &profile)?;
    let packages = plan
        .modules
        .packages
        .settings
        .resolve(&PackageCatalog::builtin()?)
        .map_or(0, |list| list.len());
    let mut reply = json!({
        "ok": cross.is_empty(), "fields": cross, "modules": artifacts_json(&modules),
        "plan": plan_file(app, &plan, &profile), "package_count": packages,
        "warnings": issues(&plan.warnings()),
    });
    if let Some(base) = body.base {
        let old: TargetPlan = app.store.configs().load(base.object_id, base.version)?;
        let old_modules = plan_modules(&old, &profile)?;
        if let Some(object) = reply.as_object_mut() {
            let _changes = object.insert("changes".to_owned(), doc_changes(&old, &plan)?);
            let _rendered = object.insert(
                "rendered_diff".to_owned(),
                rendered_diff(&old_modules, &modules),
            );
        }
    }
    Ok(reply)
}

/// `POST /configs/{kind}/preview`：草稿渲染，不落库。
///
/// # Errors
///
/// 种类不支持预览或上下文不存在时返回错误。
pub(crate) async fn preview(
    State(app): State<Arc<App>>,
    _principal: Principal,
    Path(kind): Path<String>,
    Json(body): Json<PreviewBody>,
) -> Outcome<Json<Value>> {
    match kind.as_str() {
        "profile" => profile_preview(&app, body).map(Json),
        "init_plan" => plan_preview(&app, body).map(Json),
        other => Err(Failure::bad_request(
            "NO_PREVIEW",
            format!("{other} 不支持预览"),
        )),
    }
}

/// 校验请求。
#[derive(Debug, Deserialize)]
pub(crate) struct ValidateBody {
    /// 草稿文档。
    document: Value,
}

/// 按种类校验。
fn validate_of<T: ConfigDocument + serde::de::DeserializeOwned>(document: Value) -> Value {
    match parse_or_issues::<T>(document) {
        Ok(_doc) => json!({ "ok": true, "fields": [] }),
        Err(fields) => json!({ "ok": false, "fields": fields }),
    }
}

/// `POST /configs/{kind}/validate`。
///
/// # Errors
///
/// 种类未知时返回错误。
pub(crate) async fn validate(
    _principal: Principal,
    Path(kind): Path<String>,
    Json(body): Json<ValidateBody>,
) -> Outcome<Json<Value>> {
    let reply = match kind.as_str() {
        "profile" => validate_of::<Profile>(body.document),
        "init_plan" => validate_of::<TargetPlan>(body.document),
        "boot_network" => validate_of::<xpxe_domain::BootNetwork>(body.document),
        "dhcp_rules" => validate_of::<xpxe_domain::DhcpRules>(body.document),
        "timeouts" => validate_of::<xpxe_domain::Timeouts>(body.document),
        other => return Err(Failure::not_found(format!("未知配置种类 {other}"))),
    };
    Ok(Json(reply))
}

/// 差异参数。
#[derive(Debug, Deserialize)]
pub(crate) struct DiffParams {
    /// 起始版本。
    from: u64,
    /// 目标版本。
    to: u64,
}

/// 两个版本的字段差异 + 渲染差异（Profile/初始化计划）。
///
/// # Errors
///
/// 版本不存在或渲染失败时返回错误。
fn versions_diff<T: ConfigDocument>(app: &App, id: i64, params: &DiffParams) -> Outcome<Value> {
    let old: T = app.store.configs().load(id, params.from)?;
    let new: T = app.store.configs().load(id, params.to)?;
    Ok(json!({ "changes": doc_changes(&old, &new)?, "rendered_diff": [] }))
}

/// `GET /configs/{kind}/{id}/diff?from=&to=`。
///
/// # Errors
///
/// 种类未知或版本不存在时返回错误。
pub(crate) async fn diff(
    State(app): State<Arc<App>>,
    _principal: Principal,
    Path((kind, id)): Path<(String, i64)>,
    Query(params): Query<DiffParams>,
) -> Outcome<Json<Value>> {
    let mut reply = match kind.as_str() {
        "profile" => versions_diff::<Profile>(&app, id, &params)?,
        "init_plan" => versions_diff::<TargetPlan>(&app, id, &params)?,
        "boot_network" => versions_diff::<xpxe_domain::BootNetwork>(&app, id, &params)?,
        "dhcp_rules" => versions_diff::<xpxe_domain::DhcpRules>(&app, id, &params)?,
        "timeouts" => versions_diff::<xpxe_domain::Timeouts>(&app, id, &params)?,
        "node_spec" => versions_diff::<crate::nodes::NodeDoc>(&app, id, &params)?,
        other => return Err(Failure::not_found(format!("未知配置种类 {other}"))),
    };
    let rendered = match kind.as_str() {
        "profile" => {
            let spec = sample_spec("dhcp")?;
            let old: Profile = app.store.configs().load(id, params.from)?;
            let new: Profile = app.store.configs().load(id, params.to)?;
            rendered_diff(
                &render_profile(&app, &old, spec.clone())?,
                &render_profile(&app, &new, spec)?,
            )
        }
        "init_plan" => {
            let profile = plan_profile(&app, None)?;
            let old: TargetPlan = app.store.configs().load(id, params.from)?;
            let new: TargetPlan = app.store.configs().load(id, params.to)?;
            rendered_diff(
                &plan_modules(&old, &profile)?,
                &plan_modules(&new, &profile)?,
            )
        }
        _ => json!([]),
    };
    if let Some(object) = reply.as_object_mut() {
        let _old = object.insert("rendered_diff".to_owned(), rendered);
    }
    Ok(Json(reply))
}

/// 引导地址中的 IPv4（不是 IPv4 字面量时为 `0.0.0.0`）。
fn boot_host(app: &App) -> Ipv4Addr {
    let base = app.settings.boot_base.as_str();
    let rest = base.split_once("://").map_or(base, |(_, tail)| tail);
    let host = rest.split(['/', ':']).next().unwrap_or_default();
    host.parse().unwrap_or(Ipv4Addr::UNSPECIFIED)
}

/// `GET /bootnet/snippets`：现网 DHCP 配置片段 + 最近一次探测结论。
pub(crate) async fn snippets(State(app): State<Arc<App>>, _principal: Principal) -> Json<Value> {
    let items: Vec<Value> =
        xpxe_render::dhcp_snippets::render_snippets(boot_host(&app), &app.settings.boot_base)
            .into_iter()
            .map(|snippet| json!({ "kind": snippet.kind.as_str(), "text": snippet.text }))
            .collect();
    let external = app
        .volatile()
        .heartbeat
        .as_ref()
        .and_then(|record| record.heartbeat.probe.as_ref())
        .and_then(|probe| probe.external);
    Json(json!({ "server": boot_host(&app), "snippets": items, "external": external }))
}
