//! 资产：列表、上传（iPXE 固件，按种类限额）、在线获取、激活（金丝雀或书面理由）与按种类回滚。

use std::sync::Arc;

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Query, State};
use serde::Deserialize;
use serde_json::{Value, json};
use xpxe_assets::{
    AssetKind, AssetsError, Catalog, Ingest, Mirror, Operation, Source, VerifiedAsset, store_bytes,
    verify_ipxe,
};
use xpxe_store::{AssetSource, Role};

use crate::app::App;
use crate::auth::Principal;
use crate::{Failure, Outcome};

/// 资产错误 → 400/403。
fn assets_failure(error: &AssetsError) -> Failure {
    if error.code() == "forbidden" {
        Failure::forbidden("FORBIDDEN", error.to_string())
    } else {
        Failure::bad_request(&error.code().to_ascii_uppercase(), error.to_string())
    }
}

/// 目录操作者。
fn catalog_actor(principal: &Principal) -> xpxe_assets::Actor {
    xpxe_assets::Actor {
        user_id: Some(principal.user.id),
        name: principal.user.username.clone(),
        role: principal.role,
        ip: principal.ip.clone(),
    }
}

/// 解析种类。
///
/// # Errors
///
/// 未知种类时返回 400。
fn kind_of(raw: &str) -> Outcome<AssetKind> {
    raw.parse::<AssetKind>()
        .map_err(|error| assets_failure(&error))
}

/// `GET /assets`。
///
/// # Errors
///
/// 读取失败时返回错误。
pub(crate) async fn list(
    State(app): State<Arc<App>>,
    _principal: Principal,
) -> Outcome<Json<Value>> {
    let (_head, channels) = Catalog::new(&app.store, &app.settings.assets_dir())
        .channels()
        .map_err(|error| assets_failure(&error))?;
    Ok(Json(
        json!({ "assets": app.store.assets().list()?, "channels": channels }),
    ))
}

/// 登记已校验资产。
///
/// # Errors
///
/// 写入失败时返回错误。
fn register(app: &App, principal: &Principal, asset: &VerifiedAsset) -> Outcome<Value> {
    let actor = catalog_actor(principal);
    let record = Catalog::new(&app.store, &app.settings.assets_dir())
        .register(
            asset,
            Operation {
                actor: &actor,
                now: app.now(),
            },
        )
        .map_err(|error| assets_failure(&error))?;
    app.push(
        "task.progress",
        json!({ "task": "asset.ingest", "asset_id": record.id, "kind": asset.kind }),
    );
    Ok(json!(record))
}

/// `?kind=`。
#[derive(Debug, Deserialize)]
pub(crate) struct KindQuery {
    /// 资产种类。
    kind: String,
}

/// `POST /assets/upload?kind=…`：正文为文件字节（iPXE 固件），格式检查后内容寻址落盘。
///
/// # Errors
///
/// 角色不足、种类不支持上传、超限或格式不符时返回错误。
pub(crate) async fn upload(
    State(app): State<Arc<App>>,
    principal: Principal,
    Query(query): Query<KindQuery>,
    body: Bytes,
) -> Outcome<Json<Value>> {
    principal.require(Role::Operator)?;
    let kind = kind_of(&query.kind)?;
    if !kind.is_ipxe() {
        return Err(Failure::bad_request(
            "UPLOAD_UNSUPPORTED",
            "本端点只接受 iPXE 固件；netboot 与固件包请用 /assets/fetch",
        ));
    }
    let size = u64::try_from(body.len()).unwrap_or(u64::MAX);
    if size > kind.size_limit() {
        return Err(Failure::new(
            http::StatusCode::PAYLOAD_TOO_LARGE,
            "TOO_LARGE",
            "超过该种类上限",
        ));
    }
    let format = verify_ipxe(kind, &body).map_err(|error| assets_failure(&error))?;
    let (sha256, _path) = store_bytes(&app.settings.assets_dir(), &body)
        .await
        .map_err(|error| assets_failure(&error))?;
    let asset = VerifiedAsset {
        kind,
        label: format!(
            "{} {}",
            kind.file_name(),
            format.version.as_deref().unwrap_or("（无版本串）")
        ),
        source: AssetSource::Upload,
        source_url: None,
        sha256,
        size,
        kernel_abi: None,
        verify: json!({ "format": format.format, "version": format.version, "trust": "upload+manual-activation+canary" }),
    };
    register(&app, &principal, &asset).map(Json)
}

/// 在线获取请求。
#[derive(Debug, Deserialize)]
pub(crate) struct FetchBody {
    /// 种类。
    kind: String,
    /// 自定义 URL（iPXE、固件包）；空表示官方源。
    url: Option<String>,
    /// netboot 镜像根地址。
    mirror: Option<String>,
    /// netboot 套件（默认 `trixie`）。
    suite: Option<String>,
}

/// `POST /assets/fetch`。
///
/// # Errors
///
/// 角色不足、离线模式访问官方源、签名或格式校验失败时返回错误。
pub(crate) async fn fetch(
    State(app): State<Arc<App>>,
    principal: Principal,
    Json(body): Json<FetchBody>,
) -> Outcome<Json<Value>> {
    principal.require(Role::Operator)?;
    let kind = kind_of(&body.kind)?;
    let dir = app.settings.assets_dir();
    let ingest = Ingest {
        fetcher: &app.fetcher,
        trust: &app.trust,
        dir: &dir,
    };
    let source = body.url.as_deref().map_or(Source::Official, Source::Url);
    let now = app.now();
    let result = match kind {
        AssetKind::IpxeUndionly | AssetKind::IpxeEfiX64 | AssetKind::IpxeSnponlyX64 => {
            ingest.ipxe(kind, source).await
        }
        AssetKind::DebianFirmwareCpio => ingest.firmware(source, now).await,
        AssetKind::DebianNetbootAmd64 => {
            let url = body
                .mirror
                .as_deref()
                .ok_or_else(|| Failure::bad_request("MIRROR_REQUIRED", "netboot 需要 mirror"))?;
            let suite = body.suite.as_deref().unwrap_or("trixie");
            ingest
                .netboot(
                    Mirror {
                        url,
                        suite,
                        signed: true,
                    },
                    now,
                )
                .await
        }
    };
    let asset = result.map_err(|error| assets_failure(&error))?;
    register(&app, &principal, &asset).map(Json)
}

/// 激活请求。
#[derive(Debug, Deserialize)]
pub(crate) struct ActivateBody {
    /// 种类。
    kind: String,
    /// 资产 ID。
    asset_id: i64,
    /// 跳过金丝雀的书面理由。
    skip_canary_reason: Option<String>,
}

/// `POST /assets/activate`（仅 admin）。
///
/// # Errors
///
/// 权限不足、未通过金丝雀且无理由时返回错误。
pub(crate) async fn activate(
    State(app): State<Arc<App>>,
    principal: Principal,
    Json(body): Json<ActivateBody>,
) -> Outcome<Json<Value>> {
    let kind = kind_of(&body.kind)?;
    let actor = catalog_actor(&principal);
    let version = Catalog::new(&app.store, &app.settings.assets_dir())
        .activate(
            kind,
            body.asset_id,
            body.skip_canary_reason.as_deref(),
            Operation {
                actor: &actor,
                now: app.now(),
            },
        )
        .map_err(|error| assets_failure(&error))?;
    app.bump_directives();
    app.push(
        "config.version",
        json!({ "kind": "asset_channels", "version": version }),
    );
    Ok(Json(json!(version)))
}

/// 回滚请求。
#[derive(Debug, Deserialize)]
pub(crate) struct RollbackBody {
    /// 种类。
    kind: String,
}

/// `POST /assets/rollback`（仅 admin）。
///
/// # Errors
///
/// 权限不足或没有历史时返回错误。
pub(crate) async fn rollback(
    State(app): State<Arc<App>>,
    principal: Principal,
    Json(body): Json<RollbackBody>,
) -> Outcome<Json<Value>> {
    let kind = kind_of(&body.kind)?;
    let actor = catalog_actor(&principal);
    let version = Catalog::new(&app.store, &app.settings.assets_dir())
        .rollback(
            kind,
            Operation {
                actor: &actor,
                now: app.now(),
            },
        )
        .map_err(|error| assets_failure(&error))?;
    app.bump_directives();
    Ok(Json(json!(version)))
}
