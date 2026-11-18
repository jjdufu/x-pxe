//! arm / rearm：前置检查 → 签发会话（固定版本）→ 渲染并缓存产物 → 状态转为 `Armed`。

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use serde::Deserialize;
use serde_json::{Value, json};
use xpxe_domain::init::PlainPath;
use xpxe_domain::{
    BootChannel, BootToken, EventEnvelope, EventId, EventSource, HttpUrl, NodeEvent, Ttl,
};
use xpxe_render::init::{DEFAULT_LOG_PATH, PlanContext, render_plan, sha256_hex};
use xpxe_render::{
    ArmedState, RenderRequest, RenderRequestParts, RequestDigests, RequestTokens, render,
};
use xpxe_store::{IssuedSession, Pinned, Role, SessionRequest};

use crate::app::App;
use crate::artifacts::{self, AGENT_TOKEN, BOOT_TOKEN, META, SessionMeta};
use crate::auth::Principal;
use crate::precheck::{self, Checked};
use crate::transition::{Incoming, current_ttl, deliver, fresh_id};
use crate::{Outcome, internal};

/// 会话总时长：四个超时之和（agent 令牌须覆盖到 `Configuring` 结束）。
const fn session_ttl(ttl: &Ttl) -> std::time::Duration {
    ttl.arm()
        .saturating_add(ttl.install())
        .saturating_add(ttl.firstboot())
        .saturating_add(ttl.configure())
}

/// 从 netboot 资产的校验信息取成员文件哈希。
fn member(checked: &Checked, name: &str) -> String {
    checked
        .netboot
        .meta
        .verify
        .get("files")
        .and_then(|files| files.get(name))
        .and_then(|file| file.get("sha256"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

/// 文本产物条目。
fn entry(name: &str, text: &str) -> (String, Vec<u8>) {
    (name.to_owned(), text.as_bytes().to_vec())
}

/// 会话元数据（引导端据此服务内核、initrd 与固件）。
fn session_meta(checked: &Checked, issued: &IssuedSession) -> SessionMeta {
    SessionMeta {
        node_id: checked.row.id,
        channel: channel_name(checked).to_owned(),
        linux: member(checked, "linux"),
        initrd: member(checked, "initrd.gz"),
        firmware_cpio: checked
            .firmware_cpio
            .as_ref()
            .map(|record| record.meta.sha256.clone()),
        firmware: checked.firmware.clone(),
        expires_at: issued.expires_at,
    }
}

/// 渲染全部产物并写入会话目录。
///
/// # Errors
///
/// 渲染或写入失败时返回错误。
fn render_artifacts(
    app: &App,
    checked: &Checked,
    issued: &IssuedSession,
    ttl: &Ttl,
) -> Outcome<()> {
    let base = app.settings.boot_base.as_str();
    let agent_token = BootToken::parse(issued.agent.as_str()).map_err(internal)?;
    let plan = render_plan(
        &checked.plan.1,
        &checked.profile.1,
        &PlanContext {
            session: issued.session.clone(),
            events_url: HttpUrl::parse(&format!("{base}/p/{}/events", agent_token.as_str()))
                .map_err(internal)?,
            log_path: PlainPath::parse(DEFAULT_LOG_PATH).map_err(internal)?,
            final_retry_s: ttl.configure().as_secs(),
        },
    )
    .map_err(internal)?;
    let request = RenderRequest::from_parts(RenderRequestParts {
        profile: checked.profile.1.clone(),
        node: checked.doc.spec.clone(),
        boot_base: app.settings.boot_base.clone(),
        tokens: RequestTokens {
            boot_token: issued.boot.clone(),
            agent_token: agent_token.clone(),
            session_short: issued.session.as_str().chars().take(8).collect(),
            event_id: EventId::parse(&format!("{}.installer", issued.session.as_str()))
                .map_err(internal)?,
        },
        digests: RequestDigests {
            agent_sha256: checked.agent_sha256.clone(),
            plan_sha256: sha256_hex(plan.json.as_bytes()),
        },
        armed: ArmedState::armed(),
    });
    let rendered = render(&request).map_err(internal)?;
    let meta = session_meta(checked, issued);
    let mut files = vec![
        entry(BOOT_TOKEN, issued.boot.as_str()),
        entry(AGENT_TOKEN, agent_token.as_str()),
        entry("boot.ipxe", rendered.boot_ipxe()),
        entry("preseed.cfg", rendered.preseed()),
        entry("partman-early.sh", rendered.partman_early()),
        entry("disk-select.sh", rendered.disk_select()),
        entry("late.sh", rendered.late()),
        entry("plan.json", &plan.json),
        (
            META.to_owned(),
            serde_json::to_vec(&meta).map_err(internal)?,
        ),
    ];
    for (index, repo) in checked.profile.1.extra_repos().iter().enumerate() {
        files.push((
            format!("repo-key-{index}.asc"),
            repo.key.as_str().as_bytes().to_vec(),
        ));
    }
    artifacts::write(&app.settings, &issued.session, &files)
}

/// 通道名。
const fn channel_name(checked: &Checked) -> &'static str {
    match checked.doc.spec.channel() {
        BootChannel::Lan => "lan",
        BootChannel::Redfish { .. } => "redfish",
    }
}

/// 关闭会话并清理产物（arm 失败时回滚）。
fn abandon(app: &App, issued: &IssuedSession) {
    if let Err(error) = app
        .store
        .tokens()
        .close(&issued.session, "cancelled", app.now())
    {
        tracing::error!(%error, "回滚会话失败");
    }
    artifacts::purge(&app.settings, issued.session.as_str());
}

/// arm 或 rearm 的公共流程。
///
/// # Errors
///
/// 角色不足、前置检查失败、渲染失败或转换被拒时返回错误。
async fn arm_node(
    app: &App,
    principal: &Principal,
    node_id: i64,
    confirmation: Option<String>,
) -> Outcome<Value> {
    principal.require(Role::Operator)?;
    let checked = precheck::run(app, node_id).await?;
    let ttl = current_ttl(app)?;
    let issued = app.store.tokens().issue(
        &SessionRequest {
            node_id,
            channel: channel_name(&checked).to_owned(),
            pinned: Pinned {
                profile: checked.profile.0.clone(),
                init_plan: Some(checked.plan.0.clone()),
                asset_channels: checked.channels.clone(),
                canary_assets: Vec::new(),
            },
            ttl: session_ttl(&ttl),
        },
        app.now(),
    )?;
    if let Err(error) = render_artifacts(app, &checked, &issued, &ttl) {
        abandon(app, &issued);
        return Err(error);
    }
    let event = confirmation.map_or(NodeEvent::Arm, |text| NodeEvent::Rearm {
        confirmation: text,
    });
    let incoming = Incoming {
        node_id,
        envelope: EventEnvelope {
            id: fresh_id(app, "admin.arm", node_id)?,
            session: Some(issued.session.clone()),
            source: EventSource::Admin,
            event,
        },
        actor: principal.actor(),
        payload: json!({ "user": principal.user.username, "channel": channel_name(&checked) }),
    };
    match deliver(app, &incoming) {
        Ok(delivered) => Ok(json!({
            "from": delivered.from, "to": delivered.to, "session_id": issued.session.as_str(),
            "expires_at": issued.expires_at,
        })),
        Err(error) => {
            abandon(app, &issued);
            Err(error)
        }
    }
}

/// `POST /nodes/{id}/arm`。
///
/// # Errors
///
/// 见 [`arm_node`]。
pub(crate) async fn arm(
    State(app): State<Arc<App>>,
    principal: Principal,
    Path(id): Path<i64>,
) -> Outcome<Json<Value>> {
    arm_node(&app, &principal, id, None).await.map(Json)
}

/// rearm 请求。
#[derive(Debug, Deserialize)]
pub(crate) struct RearmBody {
    /// 主机名确认串。
    confirmation: String,
}

/// `POST /nodes/{id}/rearm`。
///
/// # Errors
///
/// 见 [`arm_node`]；确认串不符返回 409 `BAD_CONFIRMATION`。
pub(crate) async fn rearm(
    State(app): State<Arc<App>>,
    principal: Principal,
    Path(id): Path<i64>,
    Json(body): Json<RearmBody>,
) -> Outcome<Json<Value>> {
    arm_node(&app, &principal, id, Some(body.confirmation))
        .await
        .map(Json)
}
