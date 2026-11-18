//! 测试内 HTTPS 服务：测试 CA 签发的 localhost/127.0.0.1 证书，按路径返回原始 HTTP 响应。

use std::net::SocketAddr;
use std::sync::Arc;

use rustls::pki_types::pem::PemObject as _;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;

use crate::support::fixture;

/// 路由：（路径，端口）→ 原始响应字节。
pub(crate) type Route = Arc<dyn Fn(&str, u16) -> Vec<u8> + Send + Sync>;

/// 组装原始响应；`length` 为 `None` 时不发 `Content-Length`（读到连接关闭为止）。
pub(crate) fn response(
    status: &str,
    headers: &[(&str, &str)],
    body: &[u8],
    length: Option<usize>,
) -> Vec<u8> {
    let mut head = format!("HTTP/1.1 {status}\r\nConnection: close\r\n");
    for (name, value) in headers {
        head.push_str(name);
        head.push_str(": ");
        head.push_str(value);
        head.push_str("\r\n");
    }
    if let Some(len) = length {
        head.push_str("Content-Length: ");
        head.push_str(&len.to_string());
        head.push_str("\r\n");
    }
    head.push_str("\r\n");
    let mut out = head.into_bytes();
    out.extend_from_slice(body);
    out
}

/// 完整响应（`Content-Length` 与正文一致）。
pub(crate) fn ok(headers: &[(&str, &str)], body: &[u8]) -> Vec<u8> {
    response("200 OK", headers, body, Some(body.len()))
}

/// 重定向。
pub(crate) fn redirect(location: &str) -> Vec<u8> {
    response("302 Found", &[("Location", location)], b"", Some(0))
}

/// 在 127.0.0.1 随机端口启动服务（运行在当前 tokio 运行时上），返回端口。
///
/// # Errors
///
/// 证书加载或监听失败时返回错误。
pub(crate) async fn start(route: Route) -> Result<u16, Box<dyn std::error::Error>> {
    let certs: Vec<CertificateDer<'static>> =
        CertificateDer::pem_slice_iter(&fixture("tls/server.pem")?).collect::<Result<_, _>>()?;
    let key = PrivateKeyDer::from_pem_slice(&fixture("tls/server.key")?)?;
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
            let conn_route = Arc::clone(&route);
            let _conn =
                tokio::spawn(async move { serve(conn_acceptor, tcp, conn_route, port).await });
        }
    });
    Ok(port)
}

/// 处理一个连接：读请求头，写原始响应，关闭。
async fn serve(
    acceptor: TlsAcceptor,
    tcp: tokio::net::TcpStream,
    route: Route,
    port: u16,
) -> Option<()> {
    let mut tls = acceptor.accept(tcp).await.ok()?;
    let mut request = Vec::new();
    let mut buffer = [0_u8; 1024];
    while !request.windows(4).any(|window| window == b"\r\n\r\n") && request.len() < 16 * 1024 {
        let read = tls.read(&mut buffer).await.ok()?;
        if read == 0 {
            return None;
        }
        request.extend_from_slice(buffer.get(..read)?);
    }
    let text = String::from_utf8_lossy(&request);
    let path = text.split_whitespace().nth(1)?.to_owned();
    let raw = route(&path, port);
    tls.write_all(&raw).await.ok()?;
    tls.shutdown().await.ok()
}
