//! `control-stub`：控制面内部 API（`/internal/v1/bootnet/*`）的实验室替身。
//!
//! 由命令行参数构造指令快照（与 `proxydhcp` 子命令相同的允许列表语义），
//! 从 `--mode-file` 读取当前模式（`proxy_dhcp` / `external_dhcp` / `redfish_only`，
//! 文件缺失视为 `proxy_dhcp`），模式变化时快照 `rev` 加一。
//! 收到的观测与心跳逐行输出 JSON（`event` = `observation` / `heartbeat`），供
//! `lab/run.sh --bootnet` 断言。

use std::collections::BTreeMap;
use std::fmt::Display;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use axum::Router;
use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse as _, Response};
use axum::routing::{get, post};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest as _, Sha256};
use xpxe_domain::{
    BootNetwork, BootToken, DhcpRules, DirectiveSnapshot, FirmwareSet, Heartbeat, HeartbeatRecord,
    HttpUrl, MacAddr, NetIface, Observation, SessionId, Sha256Hex, SnapshotEntry, Timestamp,
    UefiFirmware,
};

use crate::common::{Args, LabError, LabResult, emit};

/// 长轮询等待上限（之后返回 304）。
const LONG_POLL: Duration = Duration::from_secs(25);
/// 指令有效期。
const DIRECTIVE_TTL: Duration = Duration::from_hours(24);

/// 任意错误转为 [`LabError`]。
fn bad(context: &str, error: impl Display) -> LabError {
    LabError(format!("{context}：{error}"))
}

/// 加锁（毒化时取回数据）。
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// 当前墙钟。
fn wall() -> Timestamp {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());
    Timestamp::from_unix_seconds(secs)
}

/// 静态配置（来自命令行）。
struct Config {
    /// `Bearer <令牌>`。
    authorization: String,
    /// 引导网卡。
    iface: NetIface,
    /// 模式文件。
    mode_file: PathBuf,
    /// 处于 `proxy_dhcp` 模式时的快照模板（`rev`、`network` 运行时改写）。
    template: DirectiveSnapshot,
    /// 固件字节（按摘要）。
    firmware: BTreeMap<String, Bytes>,
}

/// 可变状态。
struct Live {
    /// 当前快照。
    snapshot: DirectiveSnapshot,
    /// 最近一次心跳。
    heartbeat: Option<HeartbeatRecord>,
}

/// 共享状态。
struct Stub {
    /// 配置。
    config: Config,
    /// 状态。
    live: Mutex<Live>,
}

impl Stub {
    /// 按模式文件计算网络模式。
    fn network(&self) -> BootNetwork {
        let text = std::fs::read_to_string(&self.config.mode_file).unwrap_or_default();
        let iface = self.config.iface.clone();
        match text.trim() {
            "redfish_only" => BootNetwork::RedfishOnly,
            "external_dhcp" => BootNetwork::ExternalDhcp { iface },
            _ => BootNetwork::ProxyDhcp { iface },
        }
    }

    /// 模式变化时更新快照，返回当前快照。
    fn current(&self) -> DirectiveSnapshot {
        let network = self.network();
        let mut live = lock(&self.live);
        if live.snapshot.network != network {
            let rev = live.snapshot.rev.saturating_add(1);
            live.snapshot = DirectiveSnapshot {
                rev,
                network,
                ..self.config.template.clone()
            };
            let _logged = emit(
                &json!({"event": "snapshot", "rev": rev, "mode": live.snapshot.network.label()}),
            );
        }
        live.snapshot.clone()
    }

    /// 校验令牌。
    fn authorized(&self, headers: &HeaderMap) -> bool {
        headers
            .get(header::AUTHORIZATION)
            .is_some_and(|value| value.as_bytes() == self.config.authorization.as_bytes())
    }
}

/// `?rev=N`。
#[derive(Deserialize)]
struct RevQuery {
    /// 客户端已有版本。
    rev: Option<u64>,
}

/// JSON 响应。
fn json_response(value: &impl serde::Serialize) -> Response {
    match serde_json::to_vec(value) {
        Ok(body) => ([(header::CONTENT_TYPE, "application/json")], body).into_response(),
        Err(_error) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// `GET /directives`：长轮询。
async fn directives(
    State(stub): State<Arc<Stub>>,
    headers: HeaderMap,
    Query(query): Query<RevQuery>,
) -> Response {
    if !stub.authorized(&headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let deadline = Instant::now()
        .checked_add(LONG_POLL)
        .unwrap_or_else(Instant::now);
    loop {
        let snapshot = stub.current();
        if query.rev != Some(snapshot.rev) {
            return json_response(&snapshot);
        }
        if Instant::now() >= deadline {
            return StatusCode::NOT_MODIFIED.into_response();
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

/// `GET /firmware/{sha256}`。
async fn firmware(
    State(stub): State<Arc<Stub>>,
    headers: HeaderMap,
    Path(digest): Path<String>,
) -> Response {
    if !stub.authorized(&headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    stub.config.firmware.get(&digest).map_or_else(
        || StatusCode::NOT_FOUND.into_response(),
        |bytes| bytes.clone().into_response(),
    )
}

/// `POST /observations`。
async fn observations(State(stub): State<Arc<Stub>>, headers: HeaderMap, body: Bytes) -> Response {
    if !stub.authorized(&headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let Ok(list) = serde_json::from_slice::<Vec<Observation>>(&body) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    for observation in list {
        if let Ok(mut value) = serde_json::to_value(&observation) {
            if let Some(map) = value.as_object_mut() {
                let _old = map.insert("event".to_owned(), json!("observation"));
            }
            let _logged = emit(&value);
        }
    }
    StatusCode::NO_CONTENT.into_response()
}

/// `POST /heartbeat`。
async fn heartbeat_post(
    State(stub): State<Arc<Stub>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if !stub.authorized(&headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let Ok(beat) = serde_json::from_slice::<Heartbeat>(&body) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let _logged = emit(&json!({"event": "heartbeat", "heartbeat": &beat}));
    lock(&stub.live).heartbeat = Some(HeartbeatRecord {
        received_at: wall(),
        heartbeat: beat,
    });
    StatusCode::NO_CONTENT.into_response()
}

/// `GET /heartbeat`。
async fn heartbeat_get(State(stub): State<Arc<Stub>>, headers: HeaderMap) -> Response {
    if !stub.authorized(&headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let record = lock(&stub.live).heartbeat.clone();
    record.map_or_else(
        || StatusCode::NOT_FOUND.into_response(),
        |value| json_response(&value),
    )
}

/// 读取固件文件并登记摘要。
///
/// # Errors
///
/// 读取失败。
fn load_firmware(path: &str, store: &mut BTreeMap<String, Bytes>) -> LabResult<Sha256Hex> {
    let bytes = std::fs::read(path)?;
    let actual: [u8; 32] = Sha256::digest(&bytes).into();
    let digest = Sha256Hex::from_digest(&actual);
    let _old = store.insert(digest.as_str().to_owned(), Bytes::from(bytes));
    Ok(digest)
}

/// 由命令行构造指令条目。
///
/// # Errors
///
/// MAC 或令牌非法。
fn entries(args: &Args, set: &FirmwareSet) -> LabResult<Vec<SnapshotEntry>> {
    let snponly: Vec<MacAddr> = args
        .values("--efi-file-for")
        .iter()
        .filter_map(|pair| pair.strip_suffix("=snponly.efi"))
        .map(|mac| MacAddr::parse(mac).map_err(|error| bad("--efi-file-for", error)))
        .collect::<LabResult<_>>()?;
    let expires_at = wall().saturating_add(DIRECTIVE_TTL);
    args.values("--allow-mac")
        .iter()
        .map(|text| {
            let mac = MacAddr::parse(text).map_err(|error| bad("--allow-mac", error))?;
            let tag = text.replace(':', "");
            Ok(SnapshotEntry {
                mac,
                session: SessionId::parse(&format!("lab-{tag}"))
                    .map_err(|error| bad("session", error))?,
                boot_token: BootToken::parse(&format!("lab-{tag}"))
                    .map_err(|error| bad("token", error))?,
                firmware: set.clone(),
                uefi_firmware: snponly.contains(&mac).then_some(UefiFirmware::SnpOnlyEfi),
                expires_at,
            })
        })
        .collect()
}

/// 由命令行构造配置。
///
/// # Errors
///
/// 参数缺失或非法。
fn config(args: &Args) -> LabResult<Config> {
    let token = std::fs::read_to_string(args.required("--token-file")?)?;
    let iface =
        NetIface::parse(&args.required("--iface")?).map_err(|error| bad("--iface", error))?;
    let mut firmware = BTreeMap::new();
    let set = FirmwareSet {
        undionly: load_firmware(&args.required("--undionly")?, &mut firmware)?,
        ipxe_efi: load_firmware(&args.required("--ipxe-efi")?, &mut firmware)?,
        snponly: load_firmware(&args.required("--snponly")?, &mut firmware)?,
    };
    let template = DirectiveSnapshot {
        rev: 0,
        network: BootNetwork::RedfishOnly,
        rules: DhcpRules::standard(),
        boot_base_url: HttpUrl::parse(&args.required("--boot-base")?)
            .map_err(|error| bad("--boot-base", error))?,
        active_firmware: Some(set.clone()),
        directives: entries(args, &set)?,
        acknowledged_conflicts: Vec::new(),
    };
    Ok(Config {
        authorization: format!("Bearer {}", token.trim()),
        iface,
        mode_file: PathBuf::from(args.required("--mode-file")?),
        template,
        firmware,
    })
}

/// 子命令入口。
///
/// 参数：`--listen <ip:port>`（默认 `127.0.0.1:8081`）、`--token-file`、`--iface`、`--boot-base`、
/// `--undionly`/`--ipxe-efi`/`--snponly <文件>`、`--allow-mac <MAC>`（可多次）、
/// `--efi-file-for <MAC>=snponly.efi`、`--mode-file <文件>`。
///
/// # Errors
///
/// 参数或监听失败时返回 [`LabError`]。
pub(crate) fn run(args: &Args) -> LabResult {
    let listen: SocketAddr = args.parsed("--listen", SocketAddr::from(([127, 0, 0, 1], 8081)))?;
    let config = config(args)?;
    let snapshot = config.template.clone();
    let stub = Arc::new(Stub {
        config,
        live: Mutex::new(Live {
            snapshot,
            heartbeat: None,
        }),
    });
    let runtime = crate::common::runtime()?;
    runtime.block_on(async move {
        let app = Router::new()
            .route("/internal/v1/bootnet/directives", get(directives))
            .route("/internal/v1/bootnet/firmware/{digest}", get(firmware))
            .route("/internal/v1/bootnet/observations", post(observations))
            .route(
                "/internal/v1/bootnet/heartbeat",
                post(heartbeat_post).get(heartbeat_get),
            )
            .with_state(stub);
        let listener = tokio::net::TcpListener::bind(listen).await?;
        emit(&json!({"event": "listening", "addr": listen.to_string()}))?;
        axum::serve(listener, app).await?;
        Ok(())
    })
}
