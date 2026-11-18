//! 管理端 Router：`/api/v1/*` + `WebUI` 静态文件；安全响应头、请求体上限、SSE。

use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use axum::Router;
use axum::extract::{DefaultBodyLimit, Request, State};
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::{delete, get, post};
use futures_util::stream::{self, Stream, StreamExt as _};
use http::{HeaderMap, HeaderValue, header};
use serde_json::{Value, json};
use tokio::sync::broadcast::error::RecvError;

use crate::app::App;
use crate::auth::Principal;
use crate::hub::Pushed;
use crate::{Failure, Outcome, arm, assets, auth, bmc, configs, nodes, web};

/// JSON 请求体上限。
const JSON_LIMIT: usize = 1024 * 1024;
/// 上传上限（iPXE 固件最大 4 MiB）。
const UPLOAD_LIMIT: usize = 4 * 1024 * 1024;

/// 安全响应头。
async fn security_headers(State(app): State<Arc<App>>, request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    let _csp = headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("default-src 'self'; frame-ancestors 'none'"),
    );
    let _nosniff = headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    let _referrer = headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    if app.settings.tls.is_some() {
        let _hsts = headers.insert(
            header::STRICT_TRANSPORT_SECURITY,
            HeaderValue::from_static("max-age=31536000; includeSubDomains"),
        );
    }
    response
}

/// 推送 → SSE 事件。
fn to_event(pushed: &Pushed) -> Event {
    Event::default()
        .id(pushed.id.to_string())
        .event(pushed.kind.clone())
        .data(json!({ "kind": pushed.kind, "data": pushed.data }).to_string())
}

/// `GET /events/stream`：需要会话；15 s 注释行保活；`Last-Event-ID` 补发。
async fn stream_events(
    State(app): State<Arc<App>>,
    _principal: Principal,
    headers: HeaderMap,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let last = headers
        .get("last-event-id")
        .and_then(|value| value.to_str().ok())
        .and_then(|raw| raw.parse::<u64>().ok());
    let (replay, subscription) = app.hub.subscribe(last);
    let replayed = stream::iter(replay.into_iter().map(|pushed| Ok(to_event(&pushed))));
    let live = stream::unfold(subscription, async move |mut receiver| {
        loop {
            match receiver.recv().await {
                Ok(pushed) => return Some((Ok(to_event(&pushed)), receiver)),
                Err(RecvError::Lagged(_skipped)) => {}
                Err(RecvError::Closed) => return None,
            }
        }
    });
    Sse::new(replayed.chain(live)).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keepalive"),
    )
}

/// `GET /bootnet/status`。
async fn bootnet_status(State(app): State<Arc<App>>, _principal: Principal) -> Json<Value> {
    let record = app.volatile().heartbeat.clone();
    let healthy = record
        .as_ref()
        .is_some_and(|beat| beat.is_healthy(app.now(), 30));
    Json(json!({ "heartbeat": record, "healthy": healthy, "rev": *app.rev.borrow() }))
}

/// `GET /bootnet/observations`。
///
/// # Errors
///
/// 读取失败时返回错误。
async fn bootnet_observations(
    State(app): State<Arc<App>>,
    _principal: Principal,
) -> Outcome<Json<Value>> {
    Ok(Json(json!(app.store.observations().recent(200)?)))
}

/// `GET /audit/verify`。
///
/// # Errors
///
/// 读取失败时返回错误。
async fn audit_verify(State(app): State<Arc<App>>, principal: Principal) -> Outcome<Json<Value>> {
    principal.require(xpxe_store::Role::Admin)?;
    let report = app.store.audit().verify()?;
    Ok(Json(
        json!({ "checked": report.checked, "first_broken": report.first_broken }),
    ))
}

/// `GET /system/info`：只读展示静态配置。
async fn system_info(State(app): State<Arc<App>>, _principal: Principal) -> Json<Value> {
    let settings = &app.settings;
    Json(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "boot_base_url": settings.boot_base.as_str(),
        "admin_listen": settings.admin_listen.to_string(),
        "boot_listen": settings.boot_listen.to_string(),
        "internal_listen": settings.internal_listen.to_string(),
        "data_dir": settings.data_dir.display().to_string(),
        "https": settings.tls.is_some(),
        "offline": settings.offline,
    }))
}

/// 未匹配的 `/api/*`。
async fn api_not_found() -> Failure {
    Failure::not_found("API 不存在")
}

/// `/api/v1` 路由。
fn api() -> Router<Arc<App>> {
    Router::new()
        .route("/auth/login", post(auth::login))
        .route("/auth/logout", post(auth::logout))
        .route("/me", get(auth::me))
        .route("/me/password", post(auth::change_password))
        .route("/me/tokens", post(auth::issue_token))
        .route("/me/tokens/{id}", delete(auth::revoke_token))
        .route("/users", post(auth::create_user))
        .route("/nodes", get(nodes::list).post(nodes::register))
        .route("/nodes/{id}", get(nodes::detail))
        .route("/nodes/{id}/events", get(nodes::events))
        .route("/nodes/{id}/arm", post(arm::arm))
        .route("/nodes/{id}/rearm", post(arm::rearm))
        .route("/nodes/{id}/disarm", post(nodes::disarm))
        .route("/nodes/{id}/reset", post(nodes::reset))
        .route("/nodes/{id}/redfish/deploy", post(bmc::deploy))
        .route("/nodes/{id}/redfish/eject", post(bmc::eject))
        .route("/nodes/{id}/redfish/power", post(bmc::power))
        .route("/bmcs", get(bmc::list).post(bmc::create))
        .route("/bmcs/{id}/probe", post(bmc::probe_bmc))
        .route("/bmcs/{id}/trust", post(bmc::trust))
        .route("/configs/{kind}", get(configs::list).post(configs::create))
        .route("/configs/{kind}/{id}", get(configs::get).put(configs::save))
        .route("/configs/{kind}/{id}/versions", get(configs::versions))
        .route("/configs/{kind}/{id}/rollback", post(configs::rollback))
        .route("/assets", get(assets::list))
        .route(
            "/assets/upload",
            post(assets::upload).layer(DefaultBodyLimit::max(UPLOAD_LIMIT)),
        )
        .route("/assets/fetch", post(assets::fetch))
        .route("/assets/activate", post(assets::activate))
        .route("/assets/rollback", post(assets::rollback))
        .route("/bootnet/status", get(bootnet_status))
        .route("/bootnet/observations", get(bootnet_observations))
        .route("/audit/verify", get(audit_verify))
        .route("/system/info", get(system_info))
        .route("/events/stream", get(stream_events))
}

/// 管理端 Router。
pub(crate) fn router(app: Arc<App>) -> Router {
    Router::new()
        .nest("/api/v1", api())
        .route("/api/{*rest}", axum::routing::any(api_not_found))
        .fallback(web::serve)
        .layer(DefaultBodyLimit::max(JSON_LIMIT))
        .layer(middleware::from_fn_with_state(
            Arc::clone(&app),
            security_headers,
        ))
        .with_state(app)
}
