//! ISO 库：列表（含被哪些菜单条目引用）、目录树（供路径候选）与删除（级联清理抽取产物）。

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use serde_json::{Value, json};
use xpxe_image::iso_read::IsoReader;
use xpxe_store::Role;

use crate::app::App;
use crate::auth::Principal;
use crate::{Failure, Outcome, internal};

/// 目录树最多返回的条目数。
const TREE_LIMIT: usize = 20_000;

/// `GET /isos`。
///
/// # Errors
///
/// 读取失败时返回错误。
pub(crate) async fn list(
    State(app): State<Arc<App>>,
    _principal: Principal,
) -> Outcome<Json<Value>> {
    let menu = super::current_menu(&app)?;
    let mut out = Vec::new();
    for iso in app.store.isos().list()? {
        let sha = super::sha_param(&iso.sha256)?;
        let extracts = app.store.isos().extracts(&iso.sha256)?;
        out.push(
            json!({ "iso": iso, "referenced_by": menu.referencing(&sha), "extracts": extracts }),
        );
    }
    Ok(Json(json!({
        "items": out,
        "usage_bytes": super::usage(&app)?,
        "quota_bytes": app.settings.iso_quota_bytes,
        "max_bytes": app.settings.iso_max_bytes,
    })))
}

/// 列出 ISO 中的全部条目。
///
/// # Errors
///
/// 读取或解析失败时返回错误。
fn walk(path: &std::path::Path) -> Result<Vec<Value>, String> {
    let file = std::fs::File::open(path).map_err(|error| error.to_string())?;
    let mut reader =
        IsoReader::open(std::io::BufReader::new(file)).map_err(|error| error.to_string())?;
    let entries = reader.walk().map_err(|error| error.to_string())?;
    Ok(entries
        .into_iter()
        .take(TREE_LIMIT)
        .map(|entry| json!({ "path": entry.path, "dir": entry.is_dir, "size": entry.size }))
        .collect())
}

/// `GET /isos/{sha}/tree`：路径候选。
///
/// # Errors
///
/// ISO 不存在（404）或解析失败（422）时返回错误。
pub(crate) async fn tree(
    State(app): State<Arc<App>>,
    _principal: Principal,
    Path(raw): Path<String>,
) -> Outcome<Json<Value>> {
    let sha = super::sha_param(&raw)?;
    let iso = app
        .store
        .isos()
        .live(sha.as_str())?
        .ok_or_else(|| Failure::not_found("ISO 不存在"))?;
    let path = app.settings.manual_iso_dir().join(&iso.sha256);
    let entries = tokio::task::spawn_blocking(move || walk(&path))
        .await
        .map_err(internal)?
        .map_err(|error| {
            Failure::bad_request("ISO_UNREADABLE", format!("无法读取 ISO 目录：{error}"))
        })?;
    Ok(Json(
        json!({ "sha256": iso.sha256, "entries": entries, "detection": iso.detection }),
    ))
}

/// `DELETE /isos/{sha}`：仅 admin；引用它的条目随后渲染为“不可用”。
///
/// # Errors
///
/// 角色不足、ISO 不存在或删除失败时返回错误。
pub(crate) async fn delete(
    State(app): State<Arc<App>>,
    principal: Principal,
    Path(raw): Path<String>,
) -> Outcome<Json<Value>> {
    principal.require(Role::Admin)?;
    let sha = super::sha_param(&raw)?;
    let referenced = super::current_menu(&app)?.referencing(&sha);
    let removed = app.store.isos().delete(sha.as_str(), app.now())?;
    for extract in &removed {
        if app.store.isos().extract_by_sha(extract)?.is_none() {
            let _gone =
                tokio::fs::remove_file(app.settings.manual_extract_dir().join(extract)).await;
        }
    }
    if app.store.isos().live(sha.as_str())?.is_none() {
        let _gone = tokio::fs::remove_file(app.settings.manual_iso_dir().join(sha.as_str())).await;
    }
    let detail = json!({ "sha256": sha.as_str(), "referenced_by": referenced, "extracts_removed": removed.len() });
    app.audit(&principal.actor().record(
        "iso.delete",
        &format!("iso:{}", sha.as_str()),
        "ok",
        detail.clone(),
    ));
    app.push("iso.deleted", detail.clone());
    Ok(Json(detail))
}
