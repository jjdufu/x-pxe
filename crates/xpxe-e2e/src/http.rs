//! 极简 HTTP/1.1 客户端：每个请求一条连接（`Connection: close`），可选 TLS，支持分块编码响应。

use std::net::SocketAddr;
use std::sync::Arc;

use crate::Outcome;
use rustls::pki_types::pem::PemObject as _;
use rustls::pki_types::{CertificateDer, ServerName};
use serde_json::Value;
use tokio::io::{AsyncRead, AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use xpxe_test_support::TestFailure;

/// 一个响应。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    /// 状态码。
    pub status: u16,
    /// 响应头（名称小写）。
    pub headers: Vec<(String, String)>,
    /// 正文（已解分块）。
    pub body: Vec<u8>,
}

impl Reply {
    /// 取第一个同名响应头。
    #[must_use]
    pub fn header(&self, name: &str) -> Option<&str> {
        let wanted = name.to_ascii_lowercase();
        self.headers
            .iter()
            .find(|(key, _)| *key == wanted)
            .map(|(_, value)| value.as_str())
    }

    /// 正文文本。
    #[must_use]
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    /// 正文 JSON。
    ///
    /// # Errors
    ///
    /// 正文不是 JSON 时返回错误。
    pub fn json(&self) -> Outcome<Value> {
        serde_json::from_slice(&self.body).map_err(|error| {
            format!("非 JSON 响应（{}）：{error}：{}", self.status, self.text()).into()
        })
    }
}

/// 一个请求。
#[derive(Debug, Clone, Default)]
pub struct Call<'req> {
    /// 方法。
    pub method: &'req str,
    /// 路径（含查询串）。
    pub path: &'req str,
    /// 额外请求头。
    pub headers: Vec<(&'req str, String)>,
    /// 正文。
    pub body: Vec<u8>,
}

/// 只信任测试 CA 的 TLS 客户端配置。
///
/// # Errors
///
/// CA 文件不可用时返回错误。
pub fn tls_config(ca_pem: &[u8]) -> Outcome<Arc<rustls::ClientConfig>> {
    let mut roots = rustls::RootCertStore::empty();
    for cert in CertificateDer::pem_slice_iter(ca_pem) {
        roots.add(cert?)?;
    }
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()?
    .with_root_certificates(roots)
    .with_no_client_auth();
    Ok(Arc::new(config))
}

/// 发送请求。
///
/// # Errors
///
/// 连接、TLS 握手或响应解析失败时返回错误。
pub async fn send(
    addr: SocketAddr,
    tls: Option<&Arc<rustls::ClientConfig>>,
    call: &Call<'_>,
) -> Outcome<Reply> {
    let tcp = TcpStream::connect(addr).await?;
    match tls {
        None => exchange(tcp, addr, call).await,
        Some(config) => {
            let name = ServerName::from(addr.ip());
            let stream = TlsConnector::from(Arc::clone(config))
                .connect(name, tcp)
                .await?;
            exchange(stream, addr, call).await
        }
    }
}

/// 写请求并读完整响应。
///
/// # Errors
///
/// IO 或解析失败时返回错误。
async fn exchange<S: AsyncRead + AsyncWrite + Unpin>(
    mut stream: S,
    addr: SocketAddr,
    call: &Call<'_>,
) -> Outcome<Reply> {
    let mut head = format!(
        "{} {} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\nContent-Length: {}\r\n",
        call.method,
        call.path,
        call.body.len()
    );
    for (name, value) in &call.headers {
        head = format!("{head}{name}: {value}\r\n");
    }
    head = format!("{head}\r\n");
    stream.write_all(head.as_bytes()).await?;
    stream.write_all(&call.body).await?;
    stream.flush().await?;
    let mut raw = Vec::new();
    // TLS 对端直接断开时 rustls 报 UnexpectedEof；此时已读到的数据仍然有效。
    if let Err(error) = stream.read_to_end(&mut raw).await
        && error.kind() != std::io::ErrorKind::UnexpectedEof
    {
        return Err(error.into());
    }
    parse(&raw)
}

/// 解析原始响应。
///
/// # Errors
///
/// 格式不符时返回错误。
pub fn parse(raw: &[u8]) -> Outcome<Reply> {
    let split = raw
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(|| TestFailure::new("响应缺少头部结束标记"))?;
    let head = String::from_utf8_lossy(raw.get(..split).unwrap_or_default()).into_owned();
    let rest = raw.get(split.saturating_add(4)..).unwrap_or_default();
    let mut lines = head.split("\r\n");
    let status_line = lines.next().unwrap_or_default();
    let status = status_line
        .split(' ')
        .nth(1)
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(|| TestFailure::new(format!("状态行无法解析：{status_line}")))?;
    let headers: Vec<(String, String)> = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_owned()))
        .collect();
    let chunked = headers
        .iter()
        .any(|(name, value)| name == "transfer-encoding" && value.eq_ignore_ascii_case("chunked"));
    let body = if chunked {
        dechunk(rest)?
    } else {
        rest.to_vec()
    };
    Ok(Reply {
        status,
        headers,
        body,
    })
}

/// 解分块编码。
///
/// # Errors
///
/// 分块格式不符时返回错误。
fn dechunk(mut rest: &[u8]) -> Outcome<Vec<u8>> {
    let mut out = Vec::new();
    loop {
        let line_end = rest
            .windows(2)
            .position(|window| window == b"\r\n")
            .ok_or_else(|| TestFailure::new("分块长度行缺失"))?;
        let size_text =
            String::from_utf8_lossy(rest.get(..line_end).unwrap_or_default()).into_owned();
        let size =
            usize::from_str_radix(size_text.split(';').next().unwrap_or_default().trim(), 16)?;
        let start = line_end.saturating_add(2);
        if size == 0 {
            return Ok(out);
        }
        let end = start.saturating_add(size);
        out.extend_from_slice(
            rest.get(start..end)
                .ok_or_else(|| TestFailure::new("分块被截断"))?,
        );
        rest = rest.get(end.saturating_add(2)..).unwrap_or_default();
    }
}
