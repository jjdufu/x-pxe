//! 模拟 BMC（标准 UEFI 画像）：`ServiceRoot`、会话、`Systems/1`、`VirtualMedia/Cd` 插入与弹出、
//! `Boot` 覆盖与 `Reset`；记录全部请求。HTTPS 服务改编自 xpxe-redfish 集成测试。

use std::fmt::Write as _;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use rustls::pki_types::pem::PemObject as _;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;
use xpxe_e2e::Outcome;
use xpxe_test_support::fixture_bytes;

/// 会话令牌。
const TOKEN: &str = "tok-1";
/// 介质集合。
const VM: &str = "/redfish/v1/Systems/1/VirtualMedia";

/// 模拟状态：是否已插入与请求日志。
pub(crate) type Shared = Arc<Mutex<(bool, Vec<Incoming>)>>;

/// 创建模拟 BMC。
pub(crate) fn mock() -> (Shared, Handler) {
    let state: Shared = Arc::new(Mutex::new((false, Vec::new())));
    let shared = Arc::clone(&state);
    let handler: Handler = Arc::new(move |request: &Incoming| {
        shared.lock().map_or_else(
            |_poisoned| (500, Vec::new(), Value::Null),
            |mut guard| {
                guard.1.push(request.clone());
                route(&mut guard.0, request)
            },
        )
    });
    (state, handler)
}

/// 简单响应。
fn reply(status: u16, body: Value) -> Outgoing {
    (status, Vec::new(), body)
}

/// 路由。
fn route(inserted: &mut bool, request: &Incoming) -> Outgoing {
    let path = request.path.as_str();
    match (request.method.as_str(), path) {
        ("GET", "/redfish/v1") => reply(
            200,
            json!({"@odata.id": "/redfish/v1", "RedfishVersion": "1.15.0", "Vendor": "Contoso",
                "Systems": {"@odata.id": "/redfish/v1/Systems"}, "Managers": {"@odata.id": "/redfish/v1/Managers"},
                "SessionService": {"@odata.id": "/redfish/v1/SessionService"},
                "Links": {"Sessions": {"@odata.id": "/redfish/v1/SessionService/Sessions"}}}),
        ),
        ("POST", "/redfish/v1/SessionService/Sessions") => {
            let ok = request.body.get("UserName").and_then(Value::as_str) == Some("admin")
                && request.body.get("Password").and_then(Value::as_str) == Some("secret");
            if ok {
                (
                    201,
                    vec![
                        ("X-Auth-Token".to_owned(), TOKEN.to_owned()),
                        (
                            "Location".to_owned(),
                            "/redfish/v1/SessionService/Sessions/1".to_owned(),
                        ),
                    ],
                    json!({"Id": "1"}),
                )
            } else {
                reply(
                    401,
                    json!({"error": {"code": "Base.1.0.InvalidCredentials", "message": "bad"}}),
                )
            }
        }
        _other if request.token.as_deref() != Some(TOKEN) => reply(
            401,
            json!({"error": {"code": "NoValidSession", "message": "no"}}),
        ),
        _other => resources(inserted, request),
    }
}

/// 已认证资源。
fn resources(inserted: &mut bool, request: &Incoming) -> Outgoing {
    match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/redfish/v1/Systems") => reply(
            200,
            json!({"Members": [{"@odata.id": "/redfish/v1/Systems/1"}]}),
        ),
        ("GET", "/redfish/v1/Systems/1") => reply(
            200,
            json!({"Id": "1", "PowerState": "On",
            "Boot": {"BootSourceOverrideTarget": "None", "BootSourceOverrideMode": "UEFI",
                "BootSourceOverrideTarget@Redfish.AllowableValues": ["None", "Pxe", "Cd", "Hdd"]},
            "VirtualMedia": {"@odata.id": VM}}),
        ),
        ("GET", "/redfish/v1/Managers") => reply(
            200,
            json!({"Members": [{"@odata.id": "/redfish/v1/Managers/1"}]}),
        ),
        ("GET", "/redfish/v1/Managers/1") => reply(200, json!({"Id": "1"})),
        ("GET", "/redfish/v1/Systems/1/VirtualMedia") => {
            reply(200, json!({"Members": [{"@odata.id": format!("{VM}/Cd")}]}))
        }
        ("GET", "/redfish/v1/Systems/1/VirtualMedia/Cd") => reply(
            200,
            json!({"Id": "Cd", "MediaTypes": ["CD", "DVD"], "Inserted": *inserted}),
        ),
        ("POST", "/redfish/v1/Systems/1/VirtualMedia/Cd/Actions/VirtualMedia.InsertMedia") => {
            *inserted = true;
            reply(204, Value::Null)
        }
        ("POST", "/redfish/v1/Systems/1/VirtualMedia/Cd/Actions/VirtualMedia.EjectMedia") => {
            *inserted = false;
            reply(204, Value::Null)
        }
        ("POST", "/redfish/v1/Systems/1/Actions/ComputerSystem.Reset")
        | ("PATCH", "/redfish/v1/Systems/1")
        | ("DELETE", "/redfish/v1/SessionService/Sessions/1") => reply(204, Value::Null),
        _other => reply(404, Value::Null),
    }
}

/// 带凭据（Basic 或会话令牌）的请求数。
pub(crate) fn credentialed(state: &Shared) -> usize {
    state.lock().map_or(0, |guard| {
        guard
            .1
            .iter()
            .filter(|item| item.authorization.is_some() || item.token.is_some())
            .count()
    })
}

/// 日志中匹配方法与路径后缀的请求数。
pub(crate) fn count(state: &Shared, method: &str, suffix: &str) -> usize {
    state.lock().map_or(0, |guard| {
        guard
            .1
            .iter()
            .filter(|item| item.method == method && item.path.ends_with(suffix))
            .count()
    })
}

/// 收到的请求。
#[derive(Debug, Clone)]
pub(crate) struct Incoming {
    /// 方法。
    pub(crate) method: String,
    /// 路径。
    pub(crate) path: String,
    /// `Authorization` 头。
    pub(crate) authorization: Option<String>,
    /// `X-Auth-Token` 头。
    pub(crate) token: Option<String>,
    /// JSON 正文（无正文为 `Null`）。
    pub(crate) body: Value,
}

/// 处理结果：状态码、附加头、JSON 正文。
pub(crate) type Outgoing = (u16, Vec<(String, String)>, Value);

/// 处理函数。
pub(crate) type Handler = Arc<dyn Fn(&Incoming) -> Outgoing + Send + Sync>;

/// 用夹具证书 `cert`（`bmc`/`other`）在 127.0.0.1 随机端口启动服务，返回端口。
///
/// # Errors
///
/// 证书加载或监听失败时返回错误。
pub(crate) async fn start(cert: &str, handler: Handler) -> Outcome<u16> {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../xpxe-redfish");
    let certs: Vec<CertificateDer<'static>> =
        CertificateDer::pem_slice_iter(&fixture_bytes(dir, &format!("tls/{cert}.pem"))?)
            .collect::<Result<_, _>>()?;
    let key = PrivateKeyDer::from_pem_slice(&fixture_bytes(dir, &format!("tls/{cert}.key"))?)?;
    let config = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()?
    .with_no_client_auth()
    .with_single_cert(certs, key)?;
    let acceptor = TlsAcceptor::from(Arc::new(config));
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0))).await?;
    let port = listener.local_addr()?.port();
    let _server = tokio::spawn(async move {
        while let Ok((tcp, _peer)) = listener.accept().await {
            let conn_acceptor = acceptor.clone();
            let conn_handler = Arc::clone(&handler);
            let _conn = tokio::spawn(async move { serve(conn_acceptor, tcp, conn_handler).await });
        }
    });
    Ok(port)
}

/// 读到请求头结束，返回（头文本，已读到的正文部分）。
async fn read_head<S: tokio::io::AsyncRead + Unpin>(stream: &mut S) -> Option<(String, Vec<u8>)> {
    let mut raw = Vec::new();
    let mut buffer = [0_u8; 2048];
    loop {
        if let Some(at) = raw.windows(4).position(|window| window == b"\r\n\r\n") {
            let body = raw.split_off(at.checked_add(4)?);
            return Some((String::from_utf8_lossy(&raw).into_owned(), body));
        }
        let read = stream.read(&mut buffer).await.ok()?;
        if read == 0 || raw.len() > 64 * 1024 {
            return None;
        }
        raw.extend_from_slice(buffer.get(..read)?);
    }
}

/// 解析请求。
fn parse(head: &str, body: &[u8]) -> Option<Incoming> {
    let mut lines = head.lines();
    let mut first = lines.next()?.split_whitespace();
    let method = first.next()?.to_owned();
    let path = first.next()?.to_owned();
    let mut authorization = None;
    let mut token = None;
    for line in lines.filter(|line| !line.is_empty()) {
        let (name, value) = line.split_once(':')?;
        match name.trim().to_ascii_lowercase().as_str() {
            "authorization" => authorization = Some(value.trim().to_owned()),
            "x-auth-token" => token = Some(value.trim().to_owned()),
            _other => {}
        }
    }
    Some(Incoming {
        method,
        path,
        authorization,
        token,
        body: serde_json::from_slice(body).unwrap_or(Value::Null),
    })
}

/// `Content-Length`。
fn content_length(head: &str) -> usize {
    head.lines()
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _value)| name.trim().eq_ignore_ascii_case("content-length"))
        .and_then(|(_name, value)| value.trim().parse().ok())
        .unwrap_or(0)
}

/// 处理一个连接。
async fn serve(acceptor: TlsAcceptor, tcp: tokio::net::TcpStream, handler: Handler) -> Option<()> {
    let mut tls = acceptor.accept(tcp).await.ok()?;
    let (head, mut body) = read_head(&mut tls).await?;
    let want = content_length(&head);
    while body.len() < want {
        let mut buffer = vec![0_u8; want.checked_sub(body.len())?];
        let read = tls.read(&mut buffer).await.ok()?;
        if read == 0 {
            return None;
        }
        body.extend_from_slice(buffer.get(..read)?);
    }
    let request = parse(&head, &body)?;
    let (status, headers, json) = handler(&request);
    let payload = if json.is_null() {
        Vec::new()
    } else {
        json.to_string().into_bytes()
    };
    let mut out = format!(
        "HTTP/1.1 {status} X\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",
        payload.len()
    );
    for (name, value) in headers {
        write!(out, "{name}: {value}\r\n").ok()?;
    }
    out.push_str("\r\n");
    let mut bytes = out.into_bytes();
    bytes.extend_from_slice(&payload);
    tls.write_all(&bytes).await.ok()?;
    tls.shutdown().await.ok()
}
