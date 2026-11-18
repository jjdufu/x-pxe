//! 测试内 HTTPS 服务：读完整请求（含正文），交给处理函数，写 JSON 响应并关闭连接。

use std::fmt::Write as _;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use rustls::pki_types::pem::PemObject as _;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use serde_json::Value;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;
use xpxe_redfish::{BmcConfig, Credentials, Endpoint, Fingerprint, Timing};
use xpxe_test_support::{TestResult, fixture_bytes};

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

/// 测试凭据。
pub(crate) fn credentials() -> Credentials {
    Credentials::new("admin".to_owned(), "secret".to_owned())
}

/// 夹具证书的指纹（`bmc` 或 `other`）。
///
/// # Errors
///
/// 夹具缺失时返回错误。
pub(crate) fn fingerprint(name: &str) -> TestResult<Fingerprint> {
    Ok(Fingerprint::of_der(&fixture_bytes(
        env!("CARGO_MANIFEST_DIR"),
        &format!("tls/{name}.der"),
    )?))
}

/// 测试用快速时序。
pub(crate) const fn fast() -> Timing {
    Timing {
        request: Duration::from_secs(5),
        insert: Duration::from_secs(2),
        task: Duration::from_secs(2),
        poll: Duration::from_millis(20),
    }
}

/// 指向本地端口、固定 `pin` 证书指纹的配置。
///
/// # Errors
///
/// 夹具缺失或地址不合法时返回错误。
pub(crate) fn config(port: u16, pin: &str) -> TestResult<BmcConfig> {
    Ok(BmcConfig {
        endpoint: Endpoint::parse(&format!("https://127.0.0.1:{port}"))?,
        credentials: credentials(),
        pin: fingerprint(pin)?,
        profile: None,
        system_id: None,
        timing: fast(),
    })
}

/// 多线程运行时。
///
/// # Errors
///
/// 运行时无法创建时返回错误。
pub(crate) fn runtime() -> TestResult<tokio::runtime::Runtime> {
    Ok(tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?)
}

/// 用夹具证书 `cert`（`bmc`/`other`）在 127.0.0.1 随机端口启动服务，返回端口。
///
/// # Errors
///
/// 证书加载或监听失败时返回错误。
pub(crate) async fn start(cert: &str, handler: Handler) -> TestResult<u16> {
    let dir = env!("CARGO_MANIFEST_DIR");
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
