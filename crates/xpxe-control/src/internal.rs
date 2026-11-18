//! 内部 API（`/internal/v1/bootnet/*`）：只供 bootnet 使用，Bearer 令牌常量时间比较。

use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::response::{IntoResponse as _, Response};
use axum::routing::get;
use http::{HeaderMap, StatusCode, header};
use serde::Deserialize;
use serde_json::json;
use xpxe_domain::{Heartbeat, HeartbeatRecord, Observation};

use crate::app::App;
use crate::directives::{active_firmware, channels, snapshot};
use crate::{Failure, Outcome, internal};

/// 长轮询时长（客户端超时 35 s）。
const LONG_POLL: Duration = Duration::from_secs(25);

/// 常量时间比较。
fn ct_eq(left: &[u8], right: &[u8]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .fold(0_u8, |acc, (lhs, rhs)| acc | (lhs ^ rhs))
            == 0
}

/// 校验 Bearer 令牌。
///
/// # Errors
///
/// 令牌缺失或不符时返回 401。
fn check(app: &App, headers: &HeaderMap) -> Outcome<()> {
    let presented = headers
        .get(header::AUTHORIZATION)
        .map_or(&[][..], http::HeaderValue::as_bytes);
    if ct_eq(presented, app.bootnet_auth.as_bytes()) {
        Ok(())
    } else {
        Err(Failure::new(
            StatusCode::UNAUTHORIZED,
            "UNAUTHENTICATED",
            "bootnet 令牌错误",
        ))
    }
}

/// `?rev=N`。
#[derive(Debug, Deserialize)]
struct RevQuery {
    /// bootnet 已应用的版本。
    rev: Option<u64>,
}

/// `GET /directives`：版本不同立即返回，否则等待变化，25 s 后 304。
///
/// # Errors
///
/// 令牌错误或快照生成失败时返回错误。
async fn directives(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Query(query): Query<RevQuery>,
) -> Outcome<Response> {
    check(&app, &headers)?;
    let mut receiver = app.rev.subscribe();
    let current = *receiver.borrow_and_update();
    if query.rev != Some(current) {
        return Ok(axum::Json(snapshot(&app)?).into_response());
    }
    match tokio::time::timeout(LONG_POLL, receiver.changed()).await {
        Ok(Ok(())) => Ok(axum::Json(snapshot(&app)?).into_response()),
        Ok(Err(_)) | Err(_) => Ok(StatusCode::NOT_MODIFIED.into_response()),
    }
}

/// `GET /firmware/{sha256}`：只提供当前激活的 iPXE 固件。
///
/// # Errors
///
/// 令牌错误、摘要未激活或读取失败时返回错误。
async fn firmware(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(digest): Path<String>,
) -> Outcome<Response> {
    check(&app, &headers)?;
    let known = active_firmware(&channels(&app)?).is_some_and(|set| {
        [&set.undionly, &set.ipxe_efi, &set.snponly]
            .iter()
            .any(|item| item.as_str() == digest)
    });
    if !known {
        return Err(Failure::not_found("固件未激活"));
    }
    let bytes = tokio::fs::read(app.settings.assets_dir().join(&digest))
        .await
        .map_err(internal)?;
    Ok(([(header::CONTENT_TYPE, "application/octet-stream")], bytes).into_response())
}

/// 观测 → 存储行。
///
/// # Errors
///
/// 失败时返回错误。
fn to_row(observation: &Observation) -> Outcome<xpxe_store::Observation> {
    let value = serde_json::to_value(observation).map_err(internal)?;
    let text = |pointer: &str| {
        value.pointer(pointer).map_or_else(String::new, |found| {
            found
                .as_str()
                .map_or_else(|| found.to_string(), str::to_owned)
        })
    };
    let at = value
        .get("at")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or_default();
    Ok(xpxe_store::Observation {
        at: xpxe_domain::Timestamp::from_unix_seconds(at),
        mac: text("/request/mac"),
        arch: text("/request/arch"),
        user_class: text("/request/user_class"),
        kind: text("/kind"),
        detail: value.to_string(),
    })
}

/// `POST /observations`。
///
/// # Errors
///
/// 令牌错误、正文不合法或写入失败时返回错误。
async fn observations(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    body: Bytes,
) -> Outcome<StatusCode> {
    check(&app, &headers)?;
    let list: Vec<Observation> = serde_json::from_slice(&body)
        .map_err(|error| Failure::bad_request("BAD_BODY", error.to_string()))?;
    for observation in &list {
        app.store.observations().record(&to_row(observation)?)?;
    }
    if !list.is_empty() {
        app.push("bootnet.observations", json!({ "count": list.len() }));
    }
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /heartbeat`。
///
/// # Errors
///
/// 令牌错误或正文不合法时返回错误。
async fn heartbeat_post(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    body: Bytes,
) -> Outcome<StatusCode> {
    check(&app, &headers)?;
    let beat: Heartbeat = serde_json::from_slice(&body)
        .map_err(|error| Failure::bad_request("BAD_BODY", error.to_string()))?;
    let record = HeartbeatRecord {
        received_at: app.now(),
        heartbeat: beat,
    };
    let previous = app.volatile().heartbeat.replace(record.clone());
    if previous.map(|old| old.heartbeat.health) != Some(record.heartbeat.health) {
        app.push(
            "bootnet.health",
            json!({ "health": record.heartbeat.health, "detail": record.heartbeat.detail }),
        );
    }
    Ok(StatusCode::NO_CONTENT)
}

/// `GET /heartbeat`。
///
/// # Errors
///
/// 令牌错误或尚无心跳（404）时返回错误。
async fn heartbeat_get(State(app): State<Arc<App>>, headers: HeaderMap) -> Outcome<Response> {
    check(&app, &headers)?;
    let record = app
        .volatile()
        .heartbeat
        .clone()
        .ok_or_else(|| Failure::not_found("尚无心跳"))?;
    Ok(axum::Json(record).into_response())
}

/// 内部 Router。
pub(crate) fn router(app: Arc<App>) -> Router {
    Router::new()
        .route("/internal/v1/bootnet/directives", get(directives))
        .route("/internal/v1/bootnet/firmware/{digest}", get(firmware))
        .route(
            "/internal/v1/bootnet/observations",
            axum::routing::post(observations),
        )
        .route(
            "/internal/v1/bootnet/heartbeat",
            get(heartbeat_get).post(heartbeat_post),
        )
        .layer(axum::extract::DefaultBodyLimit::max(1024 * 1024))
        .with_state(app)
}
