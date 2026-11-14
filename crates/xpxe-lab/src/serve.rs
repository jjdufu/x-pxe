//! `serve`：最小 HTTP 端，提供目录中的静态文件，并记录 `POST /events/...` 的请求体。
//!
//! 每个请求输出一行 JSON（来源、方法、路径、状态、字节数），`lab/run.sh` 据此断言
//! “preseed 被取走”等事件。

use std::net::SocketAddr;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderValue, Method, StatusCode, Uri, header};
use axum::response::{IntoResponse as _, Response};
use serde_json::json;

use crate::common::{Args, LabResult, emit};

/// 请求体上限（事件 POST）。
const MAX_BODY: usize = 64 * 1024;

/// 把 URL 路径映射到根目录下的文件；拒绝 `..`。
fn resolve(root: &Path, uri_path: &str) -> Option<PathBuf> {
    let relative = Path::new(uri_path.trim_start_matches('/'));
    let mut out = root.to_path_buf();
    for component in relative.components() {
        match component {
            Component::Normal(part) => out.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    Some(out)
}

/// 处理一个请求。
async fn handle(
    State(root): State<Arc<Path>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    method: Method,
    uri: Uri,
    body: Bytes,
) -> Response {
    let path = uri.path().to_owned();
    let (status, bytes, response) = if method == Method::POST {
        let text =
            String::from_utf8_lossy(body.get(..body.len().min(MAX_BODY)).unwrap_or_default())
                .into_owned();
        let _logged =
            emit(&json!({"event": "post", "peer": peer.to_string(), "path": path, "body": text}));
        (
            StatusCode::NO_CONTENT,
            0,
            StatusCode::NO_CONTENT.into_response(),
        )
    } else if method == Method::GET || method == Method::HEAD {
        match resolve(&root, &path) {
            Some(file) => match tokio::fs::read(&file).await {
                Ok(content) => {
                    let len = content.len();
                    let payload = if method == Method::HEAD {
                        Body::empty()
                    } else {
                        Body::from(content)
                    };
                    let mut response = Response::new(payload);
                    let _old = response
                        .headers_mut()
                        .insert(header::CONTENT_LENGTH, HeaderValue::from(len));
                    (StatusCode::OK, len, response)
                }
                Err(_missing) => (
                    StatusCode::NOT_FOUND,
                    0,
                    StatusCode::NOT_FOUND.into_response(),
                ),
            },
            None => (
                StatusCode::FORBIDDEN,
                0,
                StatusCode::FORBIDDEN.into_response(),
            ),
        }
    } else {
        (
            StatusCode::METHOD_NOT_ALLOWED,
            0,
            StatusCode::METHOD_NOT_ALLOWED.into_response(),
        )
    };
    let _logged = emit(&json!({
        "event": "http",
        "peer": peer.to_string(),
        "method": method.as_str(),
        "path": path,
        "query": uri.query(),
        "status": status.as_u16(),
        "bytes": bytes,
    }));
    response
}

/// 子命令入口：`--dir <目录>`、`--listen <ip:port>`。
///
/// # Errors
///
/// 参数或监听失败时返回 [`crate::common::LabError`]。
pub(crate) fn run(args: &Args) -> LabResult {
    let root: Arc<Path> = Arc::from(PathBuf::from(args.required("--dir")?));
    let listen: SocketAddr = args.parsed("--listen", SocketAddr::from(([10, 77, 0, 1], 8080)))?;
    let runtime = crate::common::runtime()?;
    runtime.block_on(async move {
        let app = Router::new().fallback(handle).with_state(root);
        let listener = tokio::net::TcpListener::bind(listen).await?;
        emit(&json!({"event": "listening", "addr": listen.to_string()}))?;
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await?;
        Ok(())
    })
}

#[cfg(test)]
mod tests;
