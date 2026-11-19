//! ISO 分块上传（断点续传）：开始 → 按偏移追加分块 → 完成（流式 SHA-256、识别、内容寻址落盘）。

use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use axum::extract::{Path, Query, State};
use bytes::Bytes;
use http::StatusCode;
use ring::rand::{SecureRandom as _, SystemRandom};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::io::AsyncWriteExt as _;
use xpxe_domain::Sha256Hex;
use xpxe_store::{NewIso, Role, UploadRecord};

use crate::app::App;
use crate::auth::Principal;
use crate::{Failure, Outcome, internal};

/// 单个分块上限。
pub(crate) const CHUNK_LIMIT: usize = 8 * 1024 * 1024;
/// 未完成上传的保留时长。
const STALE: Duration = Duration::from_hours(24);

/// 开始上传请求。
#[derive(Debug, Deserialize)]
pub(crate) struct BeginBody {
    /// 标签。
    label: String,
    /// 总字节数。
    size: u64,
    /// 期望的 SHA-256（可选，不一致即拒收）。
    expected_sha256: Option<String>,
}

/// 上传 ID：32 位十六进制随机数。
///
/// # Errors
///
/// 随机源失败时返回 500。
fn new_id() -> Outcome<String> {
    let mut bytes = [0_u8; 16];
    SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_error| Failure::internal("随机源不可用"))?;
    Ok(hex::encode(bytes))
}

/// 校验上传 ID 形状（防止路径穿越）并读取记录。
///
/// # Errors
///
/// 形状不对或不存在时返回 404。
fn record(app: &App, upload_id: &str) -> Outcome<UploadRecord> {
    if upload_id.len() != 32 || !upload_id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(Failure::not_found("上传不存在"));
    }
    Ok(app.store.isos().upload(upload_id)?)
}

/// 部分文件路径。
fn part_path(app: &App, upload_id: &str) -> std::path::PathBuf {
    app.settings
        .iso_upload_dir()
        .join(format!("{upload_id}.part"))
}

/// 已收字节数（部分文件长度）。
///
/// # Errors
///
/// 读取失败时返回 500。
async fn received(app: &App, upload_id: &str) -> Outcome<u64> {
    match tokio::fs::metadata(part_path(app, upload_id)).await {
        Ok(meta) => Ok(meta.len()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(error) => Err(internal(error)),
    }
}

/// 清理超过 24 小时未续传的上传。
///
/// # Errors
///
/// 读取或删除失败时返回错误。
async fn purge_stale(app: &App) -> Outcome<()> {
    let now = app.now();
    for upload in app.store.isos().uploads()? {
        let idle = now
            .unix_seconds()
            .saturating_sub(upload.updated_at.unix_seconds());
        if idle > STALE.as_secs() {
            let _gone = tokio::fs::remove_file(part_path(app, &upload.id)).await;
            app.store.isos().end_upload(&upload.id)?;
        }
    }
    Ok(())
}

/// 校验标签：1~128 个非控制字符。
///
/// # Errors
///
/// 不合法时返回 422。
fn check_label(label: &str) -> Outcome<String> {
    let trimmed = label.trim();
    let count = trimmed.chars().count();
    if count == 0 || count > 128 || trimmed.chars().any(char::is_control) {
        return Err(Failure::invalid(vec![crate::FieldIssue::new(
            "/label",
            "bad_label",
            "标签须为 1~128 个非控制字符",
        )]));
    }
    Ok(trimmed.to_owned())
}

/// `POST /isos/uploads`：仅 admin；检查单文件上限与配额。
///
/// # Errors
///
/// 角色不足、参数不合法、超限或写入失败时返回错误。
pub(crate) async fn begin(
    State(app): State<Arc<App>>,
    principal: Principal,
    Json(body): Json<BeginBody>,
) -> Outcome<Json<Value>> {
    principal.require(Role::Admin)?;
    let label = check_label(&body.label)?;
    if body.size == 0 || body.size > app.settings.iso_max_bytes {
        return Err(Failure::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "TOO_LARGE",
            format!("ISO 须为 1 字节到 {} 字节", app.settings.iso_max_bytes),
        ));
    }
    let expected = body
        .expected_sha256
        .as_deref()
        .filter(|raw| !raw.trim().is_empty())
        .map(|raw| Sha256Hex::parse(raw.trim()))
        .transpose()
        .map_err(|_error| {
            Failure::invalid(vec![crate::FieldIssue::new(
                "/expected_sha256",
                "bad_sha256",
                "须为 64 位十六进制",
            )])
        })?;
    purge_stale(&app).await?;
    if super::usage(&app)?.saturating_add(body.size) > app.settings.iso_quota_bytes {
        return Err(Failure::new(
            StatusCode::INSUFFICIENT_STORAGE,
            "QUOTA_EXCEEDED",
            "ISO 配额不足（XPXE_ISO_QUOTA_BYTES），请先删除不用的 ISO",
        ));
    }
    let upload = UploadRecord {
        id: new_id()?,
        label,
        size: body.size,
        expected_sha256: expected.map(|sha| sha.as_str().to_owned()),
        created_by: principal.user.username.clone(),
        created_at: app.now(),
        updated_at: app.now(),
    };
    tokio::fs::write(part_path(&app, &upload.id), b"")
        .await
        .map_err(internal)?;
    app.store.isos().begin_upload(&upload)?;
    Ok(Json(
        json!({ "id": upload.id, "size": upload.size, "received": 0_u64, "chunk_limit": CHUNK_LIMIT }),
    ))
}

/// `GET /isos/uploads/{id}`：续传前查询已收字节数。
///
/// # Errors
///
/// 不存在时返回 404。
pub(crate) async fn progress(
    State(app): State<Arc<App>>,
    principal: Principal,
    Path(upload_id): Path<String>,
) -> Outcome<Json<Value>> {
    principal.require(Role::Admin)?;
    let upload = record(&app, &upload_id)?;
    let got = received(&app, &upload_id).await?;
    Ok(Json(
        json!({ "id": upload.id, "label": upload.label, "size": upload.size, "received": got }),
    ))
}

/// 分块偏移。
#[derive(Debug, Deserialize)]
pub(crate) struct ChunkQuery {
    /// 该分块的起始偏移，必须等于已收字节数。
    offset: u64,
}

/// `PUT /isos/uploads/{id}?offset=N`：追加一个分块。
///
/// # Errors
///
/// 偏移不符（409）、超出声明大小（422）或写入失败时返回错误。
pub(crate) async fn chunk(
    State(app): State<Arc<App>>,
    principal: Principal,
    Path(upload_id): Path<String>,
    Query(query): Query<ChunkQuery>,
    body: Bytes,
) -> Outcome<Json<Value>> {
    principal.require(Role::Admin)?;
    let upload = record(&app, &upload_id)?;
    let got = received(&app, &upload_id).await?;
    if query.offset != got {
        return Err(Failure::conflict(
            "OFFSET_MISMATCH",
            format!("已收 {got} 字节，请从该偏移续传"),
        ));
    }
    let after = got.saturating_add(u64::try_from(body.len()).map_err(internal)?);
    if after > upload.size {
        return Err(Failure::bad_request("TOO_LONG", "分块超出声明的总大小"));
    }
    let mut file = tokio::fs::OpenOptions::new()
        .append(true)
        .open(part_path(&app, &upload_id))
        .await
        .map_err(internal)?;
    file.write_all(&body).await.map_err(internal)?;
    file.flush().await.map_err(internal)?;
    app.store.isos().touch_upload(&upload_id, app.now())?;
    Ok(Json(
        json!({ "id": upload_id, "size": upload.size, "received": after }),
    ))
}

/// `DELETE /isos/uploads/{id}`：放弃上传。
///
/// # Errors
///
/// 不存在或删除失败时返回错误。
pub(crate) async fn abort(
    State(app): State<Arc<App>>,
    principal: Principal,
    Path(upload_id): Path<String>,
) -> Outcome<Json<Value>> {
    principal.require(Role::Admin)?;
    let _upload = record(&app, &upload_id)?;
    let _gone = tokio::fs::remove_file(part_path(&app, &upload_id)).await;
    app.store.isos().end_upload(&upload_id)?;
    Ok(Json(json!({ "id": upload_id, "aborted": true })))
}

/// 摘要与识别结果。
///
/// # Errors
///
/// 读取失败时返回错误。
fn hash_and_inspect(path: &std::path::Path) -> std::io::Result<(String, Value)> {
    let sha = super::hash_file(path)?;
    let detection = super::inspect(std::io::BufReader::new(std::fs::File::open(path)?));
    Ok((sha, detection))
}

/// 摘要不符：删除部分文件、结束上传并审计。
async fn reject(
    app: &App,
    actor: &crate::auth::Actor,
    upload: &UploadRecord,
    sha: &str,
    part: &std::path::Path,
) -> Failure {
    let _gone = tokio::fs::remove_file(part).await;
    if let Err(error) = app.store.isos().end_upload(&upload.id) {
        tracing::warn!(%error, "结束上传失败");
    }
    app.audit(&actor.record(
        "iso.upload",
        &format!("iso:{sha}"),
        "rejected",
        json!({ "reason": "sha256_mismatch", "expected": upload.expected_sha256, "label": upload.label }),
    ));
    Failure::invalid(vec![crate::FieldIssue::new(
        "/expected_sha256",
        "sha256_mismatch",
        "上传内容的 SHA-256 与期望值不一致，已拒收",
    )])
}

/// `POST /isos/uploads/{id}/complete`：校验摘要、识别并按内容寻址落盘。
///
/// # Errors
///
/// 未收全（409）、摘要不符（422）或落盘失败时返回错误。
pub(crate) async fn complete(
    State(app): State<Arc<App>>,
    principal: Principal,
    Path(upload_id): Path<String>,
) -> Outcome<Json<Value>> {
    principal.require(Role::Admin)?;
    let upload = record(&app, &upload_id)?;
    let got = received(&app, &upload_id).await?;
    if got != upload.size {
        return Err(Failure::conflict(
            "INCOMPLETE",
            format!("已收 {got}/{} 字节", upload.size),
        ));
    }
    let part = part_path(&app, &upload_id);
    let scan = part.clone();
    let (sha, detection) = tokio::task::spawn_blocking(move || hash_and_inspect(&scan))
        .await
        .map_err(internal)?
        .map_err(internal)?;
    let actor = principal.actor();
    if upload
        .expected_sha256
        .as_ref()
        .is_some_and(|expected| *expected != sha)
    {
        return Err(reject(&app, &actor, &upload, &sha, &part).await);
    }
    let dest = app.settings.manual_iso_dir().join(&sha);
    if tokio::fs::try_exists(&dest).await.map_err(internal)? {
        tokio::fs::remove_file(&part).await.map_err(internal)?;
    } else {
        tokio::fs::rename(&part, &dest).await.map_err(internal)?;
    }
    let iso = NewIso {
        sha256: sha.clone(),
        size: upload.size,
        label: upload.label.clone(),
        volume_id: detection
            .get("volume_id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        hybrid: detection
            .get("hybrid")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        detected_family: detection
            .get("family")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_owned(),
        detection: detection.clone(),
        uploaded_by: upload.created_by.clone(),
    };
    let id = app.store.isos().insert(&iso, app.now())?;
    app.store.isos().end_upload(&upload_id)?;
    let summary = json!({ "id": id, "sha256": sha, "size": upload.size, "label": upload.label, "family": iso.detected_family });
    app.audit(&actor.record("iso.upload", &format!("iso:{sha}"), "ok", summary.clone()));
    app.push("iso.uploaded", summary);
    Ok(Json(
        json!({ "id": id, "sha256": sha, "size": upload.size, "label": upload.label, "detection": detection }),
    ))
}
