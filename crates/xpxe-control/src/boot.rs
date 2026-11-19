//! 引导端 Router（无会话；按令牌或 MAC 判定）：`/boot.ipxe`、`/boot/mac/{mac}`、`/fw/*`、`/p/{token}/*`、`/m/*`（手动通道）、`/healthz`。

use std::net::IpAddr;
use std::sync::Arc;

use axum::Router;
use axum::extract::{DefaultBodyLimit, Path, Request, State};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse as _, Response};
use axum::routing::{get, post};
use http::{StatusCode, header};
use serde_json::json;
use xpxe_domain::{MacAddr, NodeState};

use crate::app::App;
use crate::artifacts::{self, BOOT_TOKEN};
use crate::auth::{Actor, ClientIp, Peer};
use crate::directives::{active_firmware, channels};
use crate::{Failure, Outcome, ingest, internal, served};

/// 引导端请求体上限。
const BODY_LIMIT: usize = 64 * 1024;

/// iPXE 文本响应。
pub(crate) fn ipxe(text: String) -> Response {
    ([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], text).into_response()
}

/// `exit` 脚本。
fn exit_script(reason: &str) -> Response {
    ipxe(format!(
        "#!ipxe\necho x-pxe: not armed: {reason}\nsleep 3\nexit\n"
    ))
}

/// `GET /boot.ipxe`：固定内容。
async fn generic(State(app): State<Arc<App>>) -> Response {
    ipxe(format!(
        "#!ipxe\nchain {}/boot/mac/${{net0/mac:hexhyp}}\n",
        app.settings.boot_base.as_str()
    ))
}

/// 记录 `/boot/mac` 命中；同一会话被第二个源 IP 命中时告警。
fn note_hit(app: &App, node_id: i64, session: &str, ip: &str) {
    let Ok(addr) = ip.parse::<IpAddr>() else {
        return;
    };
    let mut volatile = app.volatile();
    let first = *volatile.boot_ips.entry(session.to_owned()).or_insert(addr);
    if first == addr {
        return;
    }
    let message = format!("会话被两个源 IP 命中：{first} 与 {addr}");
    let _old = volatile.alerts.insert(node_id, message.clone());
    drop(volatile);
    let actor = Actor {
        name: "boot".to_owned(),
        ip: ip.to_owned(),
    };
    app.audit(&actor.record(
        "boot.second_ip",
        &format!("node:{node_id}"),
        "warning",
        json!({ "message": message }),
    ));
    app.push(
        "node.alert",
        json!({ "node_id": node_id, "message": message }),
    );
}

/// `GET /boot/mac/{mac}`：只读；`Armed` 返回会话链接，否则 `exit`。
///
/// # Errors
///
/// 存储读取失败时返回错误。
async fn by_mac(
    State(app): State<Arc<App>>,
    ClientIp(ip): ClientIp,
    Path(raw): Path<String>,
) -> Outcome<Response> {
    let Ok(mac) = MacAddr::parse(&raw) else {
        return Ok(exit_script("bad mac"));
    };
    let row = app.store.nodes().by_mac(mac)?;
    let armed = row.as_ref().and_then(|found| {
        let session = found
            .active_session_id
            .as_ref()
            .filter(|_| found.state == NodeState::Armed)?;
        Some((found.id, session.as_str().to_owned()))
    });
    let observation = xpxe_store::Observation {
        at: app.now(),
        mac: mac.to_colon(),
        arch: String::new(),
        user_class: String::new(),
        kind: "boot_mac".to_owned(),
        detail: json!({ "ip": ip, "armed": armed.is_some() }).to_string(),
    };
    app.store.observations().record(&observation)?;
    if armed.is_none()
        && let Some(script) = crate::manual::menu_for_mac(&app, mac, row.is_some(), &ip)?
    {
        return Ok(script);
    }
    let Some((node_id, session)) = armed else {
        return Ok(exit_script(if row.is_some() {
            "node not armed"
        } else {
            "unknown mac"
        }));
    };
    let Some(token) = artifacts::read_text(&app.settings, &session, BOOT_TOKEN)? else {
        return Ok(exit_script("session closed"));
    };
    note_hit(&app, node_id, &session, &ip);
    Ok(ipxe(format!(
        "#!ipxe\nchain {}/p/{}/boot.ipxe\n",
        app.settings.boot_base.as_str(),
        token.trim()
    )))
}

/// `GET /fw/{dir}/{file}`：当前激活的 iPXE 固件（HTTP Boot 用）。
///
/// # Errors
///
/// 文件名未知、未激活或读取失败时返回错误。
async fn firmware(
    State(app): State<Arc<App>>,
    Path((_dir, file)): Path<(String, String)>,
) -> Outcome<Response> {
    let set = active_firmware(&channels(&app)?).ok_or_else(|| Failure::not_found("固件未激活"))?;
    let digest = match file.as_str() {
        "undionly.kpxe" => set.undionly,
        "ipxe.efi" => set.ipxe_efi,
        "snponly.efi" => set.snponly,
        _ => return Err(Failure::not_found("未知固件")),
    };
    let bytes = tokio::fs::read(app.settings.assets_dir().join(digest.as_str()))
        .await
        .map_err(internal)?;
    Ok(([(header::CONTENT_TYPE, "application/octet-stream")], bytes).into_response())
}

/// `GET /healthz`。
async fn healthz() -> &'static str {
    "ok"
}

/// 每 IP 每分钟限速。
async fn rate_limit(State(app): State<Arc<App>>, request: Request, next: Next) -> Response {
    let ip = request.extensions().get::<Peer>().map(|peer| peer.0.ip());
    // 手动通道的 Range 读取（sanboot 按块读取 ISO）不计数；整份下载与脚本请求仍受限速
    let ranged =
        request.uri().path().starts_with("/m/") && request.headers().contains_key(header::RANGE);
    if let Some(addr) = ip.filter(|_| !ranged) {
        let minute = app.now().unix_seconds().div_euclid(60);
        let limit = app.options.boot_rate_per_min;
        let mut volatile = app.volatile();
        let slot = volatile.rate.entry(addr).or_insert((minute, 0));
        if slot.0 != minute {
            *slot = (minute, 0);
        }
        slot.1 = slot.1.saturating_add(1);
        let over = slot.1 > limit;
        drop(volatile);
        if over {
            return Failure::new(
                StatusCode::TOO_MANY_REQUESTS,
                "RATE_LIMITED",
                "请求过于频繁",
            )
            .into_response();
        }
    }
    next.run(request).await
}

/// 引导端 Router。
pub(crate) fn router(app: Arc<App>) -> Router {
    Router::new()
        .route("/boot.ipxe", get(generic))
        .route("/boot/mac/{mac}", get(by_mac))
        .route("/fw/{dir}/{file}", get(firmware))
        .route("/p/{token}/events", post(ingest::post_json))
        .route("/p/{token}/events/{kind}", post(ingest::post_kind))
        .route("/p/{token}/{*rest}", get(served::get))
        .route("/healthz", get(healthz))
        .merge(crate::manual::routes())
        .layer(middleware::from_fn_with_state(Arc::clone(&app), rate_limit))
        .layer(DefaultBodyLimit::max(BODY_LIMIT))
        .with_state(app)
}
