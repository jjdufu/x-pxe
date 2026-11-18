//! 测试内 HTTP 静态服务：按路径返回固定字节（模拟 Debian 镜像）。

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::Arc;

use crate::Outcome;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::{TcpListener, TcpStream};
use xpxe_test_support::fixture_bytes;

/// 路径 → 内容。
pub type Files = Arc<BTreeMap<String, Vec<u8>>>;

/// 回答一条连接。
///
/// # Errors
///
/// IO 失败时返回错误。
async fn answer(mut stream: TcpStream, files: Files) -> std::io::Result<()> {
    let mut buffer = vec![0_u8; 4096];
    let read = stream.read(&mut buffer).await?;
    let request = String::from_utf8_lossy(buffer.get(..read).unwrap_or_default()).into_owned();
    let path = request.split(' ').nth(1).unwrap_or_default().to_owned();
    let reply = files.get(&path).map_or_else(
        || b"HTTP/1.1 404 Not Found\r\nConnection: close\r\nContent-Length: 0\r\n\r\n".to_vec(),
        |body| {
            let mut out = format!(
                "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: {}\r\n\r\n",
                body.len()
            )
            .into_bytes();
            out.extend_from_slice(body);
            out
        },
    );
    stream.write_all(&reply).await?;
    stream.shutdown().await
}

/// 在 127.0.0.1 随机端口启动。
///
/// # Errors
///
/// 监听失败时返回错误。
pub async fn serve(files: Files) -> Outcome<SocketAddr> {
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0))).await?;
    let addr = listener.local_addr()?;
    let _server = tokio::spawn(async move {
        while let Ok((stream, _peer)) = listener.accept().await {
            let shared = Arc::clone(&files);
            let _conn = tokio::spawn(async move { answer(stream, shared).await });
        }
    });
    Ok(addr)
}

/// 用 xpxe-assets 夹具组装 `trixie` 镜像（`InRelease` 与 d-i `Packages.xz`）。
///
/// # Errors
///
/// 夹具缺失时返回错误。
pub fn debian_fixture() -> Outcome<Files> {
    let assets = concat!(env!("CARGO_MANIFEST_DIR"), "/../xpxe-assets");
    let mut files = BTreeMap::new();
    let _release = files.insert(
        "/debian/dists/trixie/InRelease".to_owned(),
        fixture_bytes(assets, "debian/InRelease")?,
    );
    let _packages = files.insert(
        format!("/debian/dists/trixie/{}", xpxe_assets::PACKAGES_XZ),
        fixture_bytes(assets, "debian/Packages.xz")?,
    );
    Ok(Arc::new(files))
}
