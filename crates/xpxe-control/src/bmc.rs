//! BMC 与 Redfish：登记、探测（不带凭据取证书指纹）、固定指纹、部署 ISO、弹出与电源。

use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use axum::extract::{Path, State};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use xpxe_domain::{
    BmcId, BootChannel, ConfigDocument, ConfigError, ConfigKind, FieldError, NodeState,
    field_error, migrate_current,
};
use xpxe_image::session::{BootIsoInput, build_boot_iso};
use xpxe_redfish::{
    BmcConfig, Credentials, Endpoint, Fingerprint, PowerAction, ProfileId, RedfishError, Session,
    Target, probe,
};
use xpxe_render::redfish_boot::{AutoexecInput, render_autoexec};
use xpxe_store::{Authorship, Role, VersionRef};

use crate::app::App;
use crate::artifacts::{self, BOOT_TOKEN};
use crate::auth::Principal;
use crate::configs::find_by_name;
use crate::nodes::load;
use crate::{Failure, FieldIssue, Outcome, internal};

/// BMC 配置对象（密码单独加密存放）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BmcDoc {
    /// `https://host[:port]`。
    pub(crate) endpoint: String,
    /// 用户名。
    pub(crate) username: String,
    /// 已固定的证书指纹（`sha256:…`）；未固定时不能部署。
    #[serde(default)]
    pub(crate) pin: Option<String>,
    /// 厂商画像；空表示按 `ServiceRoot.Vendor` 自动选择。
    #[serde(default)]
    pub(crate) profile: Option<String>,
    /// 指定 `ComputerSystem` ID（多系统 BMC）。
    #[serde(default)]
    pub(crate) system_id: Option<String>,
}

impl ConfigDocument for BmcDoc {
    const KIND: ConfigKind = ConfigKind::Bmc;
    const SCHEMA: u32 = 1;

    fn validate(&self) -> Result<(), Vec<FieldError>> {
        let mut errors = Vec::new();
        if Endpoint::parse(&self.endpoint).is_err() {
            errors.push(field_error(
                "/endpoint",
                "bad_endpoint",
                "BMC 地址必须形如 https://host[:port]",
            ));
        }
        if self.username.trim().is_empty() {
            errors.push(field_error("/username", "required", "用户名不能为空"));
        }
        if let Some(pin) = &self.pin
            && Fingerprint::parse(pin).is_err()
        {
            errors.push(field_error(
                "/pin",
                "bad_fingerprint",
                "指纹必须为 sha256:<64 位十六进制>",
            ));
        }
        if let Some(name) = &self.profile
            && ProfileId::from_name(name).is_none()
        {
            errors.push(field_error("/profile", "unknown_profile", "未知厂商画像"));
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    fn migrate(from_schema: u32, value: Value) -> Result<Self, ConfigError> {
        migrate_current(from_schema, value)
    }
}

/// Redfish 错误 → 502。
fn redfish_failure(error: &RedfishError) -> Failure {
    Failure::new(
        http::StatusCode::BAD_GATEWAY,
        "REDFISH_FAILED",
        format!("{}：{}", error.code(), error.message()),
    )
}

/// 按名称读取 BMC。
///
/// # Errors
///
/// 不存在时返回 404。
pub(crate) fn bmc_by_name(app: &App, name: &str) -> Outcome<(VersionRef, BmcDoc)> {
    let id = find_by_name(&app.store, ConfigKind::Bmc, name)?
        .ok_or_else(|| Failure::not_found(format!("BMC {name} 不存在")))?;
    Ok(app.store.configs().head::<BmcDoc>(id)?)
}

/// 组装连接配置。
///
/// # Errors
///
/// 未固定指纹（409）、缺少密码或字段损坏时返回错误。
fn connect_config(app: &App, version: &VersionRef, doc: &BmcDoc) -> Outcome<BmcConfig> {
    let pin = doc
        .pin
        .as_deref()
        .ok_or_else(|| Failure::conflict("BMC_NOT_PINNED", "BMC 证书指纹尚未固定"))?;
    let password = app
        .store
        .secrets()
        .get(version.object_id)?
        .ok_or_else(|| Failure::conflict("BMC_NO_PASSWORD", "BMC 密码未设置"))?;
    Ok(BmcConfig {
        endpoint: Endpoint::parse(&doc.endpoint).map_err(|error| redfish_failure(&error))?,
        credentials: Credentials::new(doc.username.clone(), password),
        pin: Fingerprint::parse(pin).map_err(|error| redfish_failure(&error))?,
        profile: doc.profile.as_deref().and_then(ProfileId::from_name),
        system_id: doc.system_id.clone(),
        timing: app.options.redfish_timing,
    })
}

/// 打开会话并发现资源。
///
/// # Errors
///
/// 配置不全或 Redfish 失败时返回错误。
pub(crate) async fn open(app: &App, bmc: &str) -> Outcome<(Session, Target)> {
    let (version, doc) = bmc_by_name(app, bmc)?;
    let config = connect_config(app, &version, &doc)?;
    let session = Session::open(&config)
        .await
        .map_err(|error| redfish_failure(&error))?;
    match session.discover().await {
        Ok(target) => Ok((session, target)),
        Err(error) => {
            let _closed = session.close().await;
            Err(redfish_failure(&error))
        }
    }
}

/// 登记请求。
#[derive(Debug, Deserialize)]
pub(crate) struct CreateBody {
    /// 名称（节点规格中的 `bmc`）。
    name: String,
    /// 地址。
    endpoint: String,
    /// 用户名。
    username: String,
    /// 密码（加密存放，从不返回）。
    password: String,
    /// 厂商画像。
    profile: Option<String>,
    /// 系统 ID。
    system_id: Option<String>,
}

/// `POST /bmcs`。
///
/// # Errors
///
/// 角色不足、名称或字段不合法时返回错误。
pub(crate) async fn create(
    State(app): State<Arc<App>>,
    principal: Principal,
    Json(body): Json<CreateBody>,
) -> Outcome<Json<Value>> {
    principal.require(Role::Operator)?;
    let _checked = BmcId::parse(&body.name).map_err(|error| {
        Failure::invalid(vec![FieldIssue::new(
            "/name",
            error.code(),
            "名称只允许字母数字、- 与 _",
        )])
    })?;
    let doc = BmcDoc {
        endpoint: body.endpoint,
        username: body.username,
        pin: None,
        profile: body.profile,
        system_id: body.system_id,
    };
    doc.validate()
        .map_err(|errors| Failure::invalid(errors.iter().map(FieldIssue::from).collect()))?;
    let author = Authorship {
        author_id: Some(principal.user.id),
        comment: "登记 BMC".to_owned(),
        at: app.now(),
    };
    let version = app.store.configs().create(&body.name, &doc, &author)?;
    app.store.secrets().put(version.object_id, &body.password)?;
    app.audit(&principal.actor().record(
        "bmc.create",
        &format!("bmc:{}", body.name),
        "ok",
        json!({ "endpoint": doc.endpoint }),
    ));
    Ok(Json(json!({ "version": version, "document": doc })))
}

/// `GET /bmcs`。
///
/// # Errors
///
/// 读取失败时返回错误。
pub(crate) async fn list(
    State(app): State<Arc<App>>,
    _principal: Principal,
) -> Outcome<Json<Value>> {
    let mut out = Vec::new();
    for info in app.store.configs().list(ConfigKind::Bmc)? {
        let (version, doc) = app.store.configs().head::<BmcDoc>(info.id)?;
        out.push(json!({ "id": info.id, "name": info.name, "version": version, "document": doc }));
    }
    Ok(Json(Value::Array(out)))
}

/// `POST /bmcs/{id}/probe`：不发送凭据，只取证书与服务信息。
///
/// # Errors
///
/// 不存在或连接失败时返回错误。
pub(crate) async fn probe_bmc(
    State(app): State<Arc<App>>,
    principal: Principal,
    Path(id): Path<i64>,
) -> Outcome<Json<Value>> {
    principal.require(Role::Operator)?;
    let (_version, doc) = app.store.configs().head::<BmcDoc>(id)?;
    let endpoint = Endpoint::parse(&doc.endpoint).map_err(|error| redfish_failure(&error))?;
    let report = probe(&endpoint, Duration::from_secs(15))
        .await
        .map_err(|error| redfish_failure(&error))?;
    Ok(Json(json!({
        "fingerprint": report.fingerprint.to_string(),
        "subject": report.certificate.subject, "issuer": report.certificate.issuer,
        "not_before": report.certificate.not_before, "not_after": report.certificate.not_after,
        "vendor": report.vendor, "redfish_version": report.redfish_version,
        "suggested_profile": report.suggested_profile.as_str(),
    })))
}

/// 固定指纹请求。
#[derive(Debug, Deserialize)]
pub(crate) struct TrustBody {
    /// 管理员核对后的指纹。
    fingerprint: String,
}

/// `POST /bmcs/{id}/trust`。
///
/// # Errors
///
/// 角色不足、指纹格式错误时返回错误。
pub(crate) async fn trust(
    State(app): State<Arc<App>>,
    principal: Principal,
    Path(id): Path<i64>,
    Json(body): Json<TrustBody>,
) -> Outcome<Json<Value>> {
    principal.require(Role::Operator)?;
    let pin = Fingerprint::parse(&body.fingerprint).map_err(|_error| {
        Failure::invalid(vec![FieldIssue::new(
            "/fingerprint",
            "bad_fingerprint",
            "指纹格式错误",
        )])
    })?;
    let (head, mut doc) = app.store.configs().head::<BmcDoc>(id)?;
    doc.pin = Some(pin.to_string());
    let author = Authorship {
        author_id: Some(principal.user.id),
        comment: "固定证书指纹".to_owned(),
        at: app.now(),
    };
    let version = app.store.configs().save(id, &doc, head.version, &author)?;
    app.audit(&principal.actor().record(
        "bmc.trust",
        &format!("bmc:{id}"),
        "ok",
        json!({ "fingerprint": pin.to_string() }),
    ));
    Ok(Json(json!({ "version": version, "document": doc })))
}

/// 节点的 BMC 名称。
///
/// # Errors
///
/// 节点不是 Redfish 通道时返回 409。
fn node_bmc(
    app: &App,
    node_id: i64,
) -> Outcome<(xpxe_store::NodeRow, crate::nodes::NodeDoc, String)> {
    let (row, doc) = load(&app.store, node_id)?;
    let BootChannel::Redfish { bmc } = doc.spec.channel() else {
        return Err(Failure::conflict("NOT_REDFISH", "节点不是 Redfish 通道"));
    };
    let name = bmc.as_str().to_owned();
    Ok((row, doc, name))
}

/// 生成会话 ISO 到 `<data>/cache/iso/<会话>.iso`，返回其 URL。
///
/// # Errors
///
/// 产物缺失、渲染或构建失败时返回错误。
fn build_iso(app: &App, row: &xpxe_store::NodeRow, doc: &crate::nodes::NodeDoc) -> Outcome<String> {
    let session = row
        .active_session_id
        .as_ref()
        .ok_or_else(|| Failure::conflict("NOT_ARMED", "节点没有活动会话"))?;
    let token_text = artifacts::read_text(&app.settings, session.as_str(), BOOT_TOKEN)?
        .ok_or_else(|| Failure::conflict("NOT_ARMED", "会话令牌已失效"))?;
    let token = xpxe_domain::BootToken::parse(token_text.trim()).map_err(internal)?;
    let meta = artifacts::meta(&app.settings, session.as_str())?
        .ok_or_else(|| Failure::internal("会话元数据缺失"))?;
    let efi = std::fs::read(
        app.settings
            .assets_dir()
            .join(meta.firmware.ipxe_efi.as_str()),
    )
    .map_err(internal)?;
    let script = render_autoexec(&AutoexecInput {
        boot_mac: doc.spec.mac(),
        network: doc.spec.network(),
        boot_base: &app.settings.boot_base,
        boot_token: &token,
    })
    .map_err(internal)?;
    let iso = build_boot_iso(&BootIsoInput {
        session_id: session.as_str(),
        created: image_time(app.now())?,
        ipxe_efi: &efi,
        autoexec: script.as_bytes(),
    })
    .map_err(internal)?;
    let path = app
        .settings
        .iso_dir()
        .join(format!("{}.iso", session.as_str()));
    std::fs::write(&path, iso).map_err(internal)?;
    Ok(format!(
        "{}/p/{}/boot.iso",
        app.settings.boot_base.as_str(),
        token.as_str()
    ))
}

/// `POST /nodes/{id}/redfish/deploy`：前置条件 → 生成 ISO → 资源发现 → 部署第 2–6 步。
///
/// # Errors
///
/// 节点不是 Armed、不是 Redfish、BMC 未固定指纹或部署失败时返回错误。
pub(crate) async fn deploy(
    State(app): State<Arc<App>>,
    principal: Principal,
    Path(id): Path<i64>,
) -> Outcome<Json<Value>> {
    principal.require(Role::Operator)?;
    let (row, doc, bmc) = node_bmc(&app, id)?;
    if row.state != NodeState::Armed {
        return Err(Failure::conflict("NOT_ARMED", "只有 Armed 节点可以部署"));
    }
    let url = build_iso(&app, &row, &doc)?;
    app.push(
        "task.progress",
        json!({ "node_id": id, "task": "redfish.deploy", "step": "iso_ready" }),
    );
    let (session, target) = open(&app, &bmc).await?;
    let mut events = Vec::new();
    let result = session.deploy(&target, &url, &mut events).await;
    let _closed = session.close().await;
    let steps: Vec<Value> = events
        .iter()
        .map(|event| json!({ "step": event.step, "ok": event.ok, "detail": event.detail }))
        .collect();
    for step in &steps {
        app.push(
            "task.progress",
            json!({ "node_id": id, "task": "redfish.deploy", "event": step }),
        );
    }
    let outcome = if result.is_ok() { "ok" } else { "failed" };
    app.audit(&principal.actor().record(
        "redfish.deploy",
        &format!("node:{id}"),
        outcome,
        json!({ "steps": steps }),
    ));
    result.map_err(|error| redfish_failure(&error))?;
    Ok(Json(json!({ "steps": steps })))
}

/// 弹出虚拟介质（发件箱与管理员共用）。
///
/// # Errors
///
/// Redfish 失败时返回错误。
pub(crate) async fn eject_node(app: &App, node_id: i64) -> Outcome<()> {
    let (_row, _doc, bmc) = node_bmc(app, node_id)?;
    let (session, target) = open(app, &bmc).await?;
    let result = session.eject(&target).await;
    let _closed = session.close().await;
    result.map_err(|error| redfish_failure(&error))
}

/// `POST /nodes/{id}/redfish/eject`。
///
/// # Errors
///
/// 角色不足或 Redfish 失败时返回错误。
pub(crate) async fn eject(
    State(app): State<Arc<App>>,
    principal: Principal,
    Path(id): Path<i64>,
) -> Outcome<Json<Value>> {
    principal.require(Role::Operator)?;
    let result = eject_node(&app, id).await;
    let outcome = if result.is_ok() { "ok" } else { "failed" };
    app.audit(&principal.actor().record(
        "redfish.eject",
        &format!("node:{id}"),
        outcome,
        json!({}),
    ));
    result.map(|()| Json(json!({ "ejected": true })))
}

/// 电源请求。
#[derive(Debug, Deserialize)]
pub(crate) struct PowerBody {
    /// `on`、`force_off`、`force_restart`。
    action: String,
}

/// `POST /nodes/{id}/redfish/power`。
///
/// # Errors
///
/// 角色不足、动作未知或 Redfish 失败时返回错误。
pub(crate) async fn power(
    State(app): State<Arc<App>>,
    principal: Principal,
    Path(id): Path<i64>,
    Json(body): Json<PowerBody>,
) -> Outcome<Json<Value>> {
    principal.require(Role::Operator)?;
    let action = match body.action.as_str() {
        "on" => PowerAction::On,
        "force_off" => PowerAction::ForceOff,
        "force_restart" => PowerAction::ForceRestart,
        other => {
            return Err(Failure::bad_request(
                "BAD_ACTION",
                format!("未知电源动作 {other}"),
            ));
        }
    };
    let (_row, _doc, bmc) = node_bmc(&app, id)?;
    let (session, target) = open(&app, &bmc).await?;
    let result = session.power(&target, action).await;
    let _closed = session.close().await;
    let outcome = if result.is_ok() { "ok" } else { "failed" };
    app.audit(&principal.actor().record(
        "redfish.power",
        &format!("node:{id}"),
        outcome,
        json!({ "action": body.action }),
    ));
    result.map_err(|error| redfish_failure(&error))?;
    Ok(Json(json!({ "action": body.action })))
}

/// 领域时间戳 → ISO 时间戳。
///
/// # Errors
///
/// 时间超出 ISO 可表示范围时返回 500。
fn image_time(now: xpxe_domain::Timestamp) -> Outcome<xpxe_image::Timestamp> {
    let secs = i64::try_from(now.unix_seconds()).map_err(internal)?;
    let moment = time::OffsetDateTime::from_unix_timestamp(secs).map_err(internal)?;
    xpxe_image::Timestamp {
        year: u16::try_from(moment.year()).map_err(internal)?,
        month: u8::from(moment.month()),
        day: moment.day(),
        hour: moment.hour(),
        minute: moment.minute(),
        second: moment.second(),
    }
    .validate()
    .map_err(internal)
}
