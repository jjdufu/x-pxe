//! 引导端手动通道 `/m/*`：菜单脚本、ISO（sanboot，支持 Range）、抽取产物与选择回调。
//!
//! 菜单关闭时全部 404；只服务被已启用条目引用的内容；不含任何令牌或配置内容。

use std::sync::Arc;

use axum::Router;
use axum::extract::{Path, Query, State};
use axum::response::Response;
use axum::routing::get;
use http::HeaderMap;
use serde::Deserialize;
use serde_json::json;
use xpxe_domain::{BootMenu, MacAddr};
use xpxe_render::manual::{MenuInput, MenuItem, render_menu};

use crate::app::App;
use crate::auth::{Actor, ClientIp};
use crate::boot::ipxe;
use crate::{Failure, Outcome, served};

/// 菜单关闭或内容不可用时一律 404。
fn hidden() -> Failure {
    Failure::not_found("手动引导菜单未开启或内容不可用")
}

/// 已开启的菜单。
///
/// # Errors
///
/// 菜单关闭时返回 404。
fn open_menu(app: &App) -> Outcome<BootMenu> {
    Some(super::current_menu(app)?)
        .filter(|menu| menu.enabled)
        .ok_or_else(hidden)
}

/// 记录观测（失败只记日志）。
fn observe(app: &App, mac: &str, kind: &str, detail: &serde_json::Value) {
    let observation = xpxe_store::Observation {
        at: app.now(),
        mac: mac.to_owned(),
        arch: String::new(),
        user_class: String::new(),
        kind: kind.to_owned(),
        detail: detail.to_string(),
    };
    if let Err(error) = app.store.observations().record(&observation) {
        tracing::warn!(%error, "写观测失败");
    }
}

/// 引导端审计操作者。
fn boot_actor(ip: &str) -> Actor {
    Actor {
        name: "boot".to_owned(),
        ip: ip.to_owned(),
    }
}

/// `/boot/mac/{mac}` 的手动分流：菜单开启且命中时返回链到菜单的脚本（并审计命中）。
///
/// # Errors
///
/// 读取失败时返回错误。
pub(crate) fn menu_for_mac(
    app: &App,
    mac: MacAddr,
    known: bool,
    ip: &str,
) -> Outcome<Option<Response>> {
    let menu = super::current_menu(app)?;
    if !menu.shows_for(mac, known) {
        return Ok(None);
    }
    observe(app, &mac.to_colon(), "manual_menu", &json!({ "ip": ip }));
    app.audit(&boot_actor(ip).record(
        "manual.menu_hit",
        &format!("mac:{}", mac.to_colon()),
        "ok",
        json!({ "known": known }),
    ));
    Ok(Some(ipxe(format!(
        "#!ipxe\nchain {}/m/menu.ipxe?mac={}\n",
        app.settings.boot_base.as_str().trim_end_matches('/'),
        mac.to_hexhyp()
    ))))
}

/// 菜单查询参数。
#[derive(Debug, Deserialize)]
struct MenuQuery {
    /// 请求者 MAC。
    mac: String,
}

/// `GET /m/menu.ipxe?mac=`。
///
/// # Errors
///
/// 菜单关闭（404）或渲染失败时返回错误。
async fn menu(State(app): State<Arc<App>>, Query(query): Query<MenuQuery>) -> Outcome<Response> {
    let menu = open_menu(&app)?;
    let mac = MacAddr::parse(&query.mac).map_err(|_error| hidden())?;
    let row = app.store.nodes().by_mac(mac)?;
    if !menu.shows_for(mac, row.is_some()) {
        return Ok(ipxe(
            "#!ipxe\necho x-pxe: manual menu not enabled for this machine\nsleep 3\nexit\n"
                .to_owned(),
        ));
    }
    let mut items = Vec::new();
    for entry in menu.entries.iter().filter(|entry| entry.enabled) {
        items.push(MenuItem {
            id: entry.id.clone(),
            title: entry.title.clone(),
            arch: entry.arch,
            action: super::extract::action_for(&app, entry)?,
        });
    }
    let script = render_menu(&MenuInput {
        boot_base: &app.settings.boot_base,
        mac,
        timeout_s: menu.timeout_s,
        default_entry: menu.default_entry.as_deref(),
        items: &items,
    })
    .map_err(|error| Failure::internal(error.to_string()))?;
    Ok(ipxe(script))
}

/// `GET /m/iso/{sha}.iso`：只服务被已启用 `sanboot` 条目引用的 ISO。
///
/// # Errors
///
/// 菜单关闭或未被引用时 404。
async fn iso(
    State(app): State<Arc<App>>,
    Path(file): Path<String>,
    headers: HeaderMap,
) -> Outcome<Response> {
    let menu = open_menu(&app)?;
    let sha = super::sha_param(file.strip_suffix(".iso").ok_or_else(hidden)?)?;
    let referenced = menu.entries.iter().any(|entry| {
        entry.enabled
            && matches!(&entry.kind, xpxe_domain::EntryKind::Sanboot { iso } if *iso == sha)
    });
    if !referenced || app.store.isos().live(sha.as_str())?.is_none() {
        return Err(hidden());
    }
    served::file(app.settings.manual_iso_dir().join(sha.as_str()), &headers).await
}

/// `GET /m/x/{sha}`：只服务被已启用条目引用的 ISO 的抽取产物。
///
/// # Errors
///
/// 菜单关闭或产物不属于被引用的 ISO 时 404。
async fn extract(
    State(app): State<Arc<App>>,
    Path(raw): Path<String>,
    headers: HeaderMap,
) -> Outcome<Response> {
    let menu = open_menu(&app)?;
    let sha = super::sha_param(&raw)?;
    let record = app
        .store
        .isos()
        .extract_by_sha(sha.as_str())?
        .ok_or_else(hidden)?;
    let source = super::sha_param(&record.key.iso_sha256)?;
    let referenced = menu
        .entries
        .iter()
        .any(|entry| entry.enabled && entry.kind.iso() == Some(&source));
    if !referenced || app.store.isos().live(source.as_str())?.is_none() {
        return Err(hidden());
    }
    served::file(
        app.settings.manual_extract_dir().join(sha.as_str()),
        &headers,
    )
    .await
}

/// 选择回调参数。
#[derive(Debug, Deserialize)]
struct SelectQuery {
    /// MAC。
    mac: String,
    /// iPXE 标签（`m-<条目 ID>` 或 `xpxe-exit`）。
    entry: String,
}

/// `GET /m/select?mac=&entry=`：记录条目被选择（观测 + 审计），返回空脚本。
///
/// # Errors
///
/// 菜单关闭时返回 404。
async fn select(
    State(app): State<Arc<App>>,
    ClientIp(ip): ClientIp,
    Query(query): Query<SelectQuery>,
) -> Outcome<Response> {
    let menu = open_menu(&app)?;
    let mac = MacAddr::parse(&query.mac).map_err(|_error| hidden())?;
    let label = query.entry.strip_prefix("m-").unwrap_or(&query.entry);
    let known = menu.entry(label).map(|entry| entry.id.clone());
    let chosen = known.clone().unwrap_or_else(|| "exit".to_owned());
    observe(
        &app,
        &mac.to_colon(),
        "manual_select",
        &json!({ "ip": ip, "entry": chosen }),
    );
    app.audit(&boot_actor(&ip).record(
        "manual.select",
        &format!("mac:{}", mac.to_colon()),
        if known.is_some() || label == "xpxe-exit" {
            "ok"
        } else {
            "unknown_entry"
        },
        json!({ "entry": chosen }),
    ));
    Ok(ipxe("#!ipxe\n".to_owned()))
}

/// 手动通道路由（挂在引导端口）。
pub(crate) fn routes() -> Router<Arc<App>> {
    Router::new()
        .route("/m/menu.ipxe", get(menu))
        .route("/m/iso/{file}", get(iso))
        .route("/m/x/{sha}", get(extract))
        .route("/m/select", get(select))
}

/// 预览请求。
#[derive(Debug, Deserialize)]
pub(crate) struct PreviewBody {
    /// 尚未保存的菜单文档。
    document: serde_json::Value,
    /// 预览用 MAC（默认取白名单第一个）。
    mac: Option<String>,
}

/// `POST /boot-menu/preview`：按未保存的文档渲染 `menu.ipxe`（供编辑页实时预览）。
///
/// # Errors
///
/// 文档不合法（422）或渲染失败时返回错误。
pub(crate) async fn preview(
    State(app): State<Arc<App>>,
    _principal: crate::auth::Principal,
    axum::Json(body): axum::Json<PreviewBody>,
) -> Outcome<axum::Json<serde_json::Value>> {
    let menu: BootMenu = crate::configs::parse_doc(body.document)?;
    let mac = match body.mac.as_deref().filter(|raw| !raw.trim().is_empty()) {
        Some(raw) => MacAddr::parse(raw.trim())
            .map_err(|error| Failure::bad_request("BAD_MAC", error.to_string()))?,
        None => menu
            .allow_macs
            .first()
            .copied()
            .map_or_else(|| MacAddr::parse("52:54:00:00:00:00"), Ok)
            .map_err(|error| Failure::internal(error.to_string()))?,
    };
    let mut items = Vec::new();
    for entry in menu.entries.iter().filter(|entry| entry.enabled) {
        items.push(MenuItem {
            id: entry.id.clone(),
            title: entry.title.clone(),
            arch: entry.arch,
            action: super::extract::action_for(&app, entry)?,
        });
    }
    let script = render_menu(&MenuInput {
        boot_base: &app.settings.boot_base,
        mac,
        timeout_s: menu.timeout_s,
        default_entry: menu.default_entry.as_deref(),
        items: &items,
    })
    .map_err(|error| Failure::internal(error.to_string()))?;
    Ok(axum::Json(
        json!({ "script": script, "mac": mac.to_colon() }),
    ))
}
