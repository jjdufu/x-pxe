//! `WebUI` 静态文件：按路径取 `XPXE_WEB_DIR` 下的文件，找不到时回退到 `index.html`（`SPA`）。

use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use axum::extract::State;
use axum::response::{IntoResponse as _, Response};
use http::{Uri, header};

use crate::Failure;
use crate::app::App;

/// 按扩展名取 MIME。
fn mime(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default()
    {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        _ => "application/octet-stream",
    }
}

/// 安全拼接：拒绝 `..` 与绝对路径。
fn resolve(root: &Path, uri_path: &str) -> Option<PathBuf> {
    let relative = Path::new(uri_path.trim_start_matches('/'));
    relative
        .components()
        .all(|part| matches!(part, Component::Normal(_)))
        .then(|| root.join(relative))
}

/// 静态文件处理器。
pub(crate) async fn serve(State(app): State<Arc<App>>, uri: Uri) -> Response {
    let root = app.settings.web_dir.clone();
    let candidate = resolve(&root, uri.path()).filter(|path| path.is_file());
    let path = candidate.unwrap_or_else(|| root.join("index.html"));
    match tokio::fs::read(&path).await {
        Ok(bytes) => ([(header::CONTENT_TYPE, mime(&path))], bytes).into_response(),
        Err(_error) => Failure::not_found("WebUI 未安装").into_response(),
    }
}
