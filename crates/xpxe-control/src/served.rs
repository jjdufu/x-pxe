//! 会话资源 `GET /p/{token}/*`：按令牌种类授权，一次性文档第二次返回 410，大文件支持 Range。

use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Path, State};
use axum::response::{IntoResponse as _, Response};
use bytes::Bytes;
use futures_util::stream;
use http::{HeaderMap, HeaderValue, StatusCode, header};
use serde_json::json;
use tokio::io::{AsyncReadExt as _, AsyncSeekExt as _};
use xpxe_domain::SessionId;
use xpxe_store::{Resource, ServedDoc, StoreError};

use crate::app::App;
use crate::artifacts::{self, SessionMeta};
use crate::auth::{Actor, ClientIp};
use crate::boot::ipxe;
use crate::{Failure, Outcome, internal};

/// 流式读取块大小。
const CHUNK: usize = 64 * 1024;

/// 令牌被拒。
fn rejected() -> Failure {
    Failure::forbidden("TOKEN_REJECTED", "令牌无效、已过期、会话已关闭或种类不符")
}

/// 授权并返回会话。
///
/// # Errors
///
/// 令牌不能访问该资源时返回 403。
fn authorize(app: &App, token: &str, resource: Resource) -> Outcome<SessionId> {
    app.store
        .tokens()
        .authorize(token, resource, app.now())?
        .ok_or_else(rejected)
}

/// 会话元数据。
///
/// # Errors
///
/// 缺失时返回 404。
fn meta(app: &App, session: &SessionId) -> Outcome<SessionMeta> {
    artifacts::meta(&app.settings, session.as_str())?
        .ok_or_else(|| Failure::not_found("会话产物缺失"))
}

/// 会话目录中的文本文件。
///
/// # Errors
///
/// 缺失时返回 404。
fn text(app: &App, session: &SessionId, name: &str) -> Outcome<Response> {
    let body = artifacts::read_text(&app.settings, session.as_str(), name)?
        .ok_or_else(|| Failure::not_found("会话产物缺失"))?;
    Ok(([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], body).into_response())
}

/// 一次性文档。
///
/// # Errors
///
/// 第二次请求返回 410 并写审计。
fn once(app: &App, session: &SessionId, doc: ServedDoc, name: &str, ip: &str) -> Outcome<Response> {
    match app.store.tokens().consume_once(session, doc, app.now()) {
        Ok(()) => text(app, session, name),
        Err(StoreError::AlreadyServed) => {
            let actor = Actor {
                name: "boot".to_owned(),
                ip: ip.to_owned(),
            };
            app.audit(&actor.record(
                "boot.replay",
                &format!("session:{}", session.as_str()),
                "denied",
                json!({ "doc": name }),
            ));
            Err(Failure::from(StoreError::AlreadyServed))
        }
        Err(error) => Err(Failure::from(error)),
    }
}

/// 解析单段 `Range: bytes=a-b`。
fn parse_range(headers: &HeaderMap, size: u64) -> Option<(u64, u64)> {
    let raw = headers
        .get(header::RANGE)?
        .to_str()
        .ok()?
        .strip_prefix("bytes=")?;
    let (start_raw, end_raw) = raw.split_once('-')?;
    let last = size.checked_sub(1)?;
    let (start, end) = if start_raw.is_empty() {
        let suffix: u64 = end_raw.parse().ok()?;
        (size.saturating_sub(suffix), last)
    } else {
        let start: u64 = start_raw.parse().ok()?;
        let end = if end_raw.is_empty() {
            last
        } else {
            end_raw.parse::<u64>().ok()?.min(last)
        };
        (start, end)
    };
    (start <= end).then_some((start, end))
}

/// 流式文件响应（支持 Range）。
///
/// # Errors
///
/// 文件缺失或读取失败时返回错误。
async fn file(path: PathBuf, headers: &HeaderMap) -> Outcome<Response> {
    let mut opened = tokio::fs::File::open(&path)
        .await
        .map_err(|_error| Failure::not_found("文件缺失"))?;
    let size = opened.metadata().await.map_err(internal)?.len();
    let range = parse_range(headers, size);
    let (start, end) = range.unwrap_or_else(|| (0, size.saturating_sub(1)));
    let length = if size == 0 {
        0
    } else {
        end.saturating_sub(start).saturating_add(1)
    };
    let _position = opened
        .seek(std::io::SeekFrom::Start(start))
        .await
        .map_err(internal)?;
    let body = Body::from_stream(stream::unfold(
        (opened, length),
        async move |(mut handle, left)| {
            if left == 0 {
                return None;
            }
            let want = usize::try_from(left).map_or(CHUNK, |value| value.min(CHUNK));
            let mut buffer = vec![0_u8; want];
            match handle.read(&mut buffer).await {
                Ok(0) => None,
                Ok(read) => {
                    buffer.truncate(read);
                    let rest = left.saturating_sub(u64::try_from(read).unwrap_or(left));
                    Some((
                        Ok::<Bytes, std::io::Error>(Bytes::from(buffer)),
                        (handle, rest),
                    ))
                }
                Err(error) => Some((Err(error), (handle, 0))),
            }
        },
    ));
    let mut response = Response::new(body);
    let response_headers = response.headers_mut();
    let _old = response_headers.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    let _len = response_headers.insert(header::CONTENT_LENGTH, HeaderValue::from(length));
    if range.is_some() {
        let value =
            HeaderValue::from_str(&format!("bytes {start}-{end}/{size}")).map_err(internal)?;
        let _range = response_headers.insert(header::CONTENT_RANGE, value);
        *response.status_mut() = StatusCode::PARTIAL_CONTENT;
    }
    Ok(response)
}

/// 资产文件。
///
/// # Errors
///
/// 摘要缺失时返回 404。
async fn asset(app: &App, pinned: Option<&str>, headers: &HeaderMap) -> Outcome<Response> {
    let digest = pinned
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Failure::not_found("资产未固定"))?;
    file(app.settings.assets_dir().join(digest), headers).await
}

/// 二进制资源：内核、initrd、固件包、agent 与 Redfish ISO（支持单段 Range）。
///
/// # Errors
///
/// 令牌被拒（403）或文件缺失（404）时返回错误。
async fn binary(app: &App, token: &str, rest: &str, headers: &HeaderMap) -> Outcome<Response> {
    match rest {
        "linux" => {
            asset(
                app,
                Some(&meta(app, &authorize(app, token, Resource::Kernel)?)?.linux),
                headers,
            )
            .await
        }
        "initrd.gz" => {
            asset(
                app,
                Some(&meta(app, &authorize(app, token, Resource::Initrd)?)?.initrd),
                headers,
            )
            .await
        }
        "firmware.cpio.gz" => {
            asset(
                app,
                meta(app, &authorize(app, token, Resource::Initrd)?)?
                    .firmware_cpio
                    .as_deref(),
                headers,
            )
            .await
        }
        "agent" => {
            // late.sh 用引导令牌下载 agent（二进制本身不含敏感信息），两种令牌都接受。
            let _resolved = app
                .store
                .tokens()
                .resolve(token, app.now())?
                .ok_or_else(rejected)?;
            file(app.settings.agent_file.clone(), headers).await
        }
        "boot.iso" => {
            let session = authorize(app, token, Resource::Iso)?;
            file(
                app.settings
                    .iso_dir()
                    .join(format!("{}.iso", session.as_str())),
                headers,
            )
            .await
        }
        _ => Err(Failure::not_found("未知资源")),
    }
}

/// `GET /p/{token}/{rest}`。
///
/// # Errors
///
/// 令牌被拒（403）、一次性文档重复（410）或文件缺失（404）时返回错误。
pub(crate) async fn get(
    State(app): State<Arc<App>>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    Path((token, rest)): Path<(String, String)>,
) -> Outcome<Response> {
    match rest.as_str() {
        "boot.ipxe" => {
            let session = authorize(&app, &token, Resource::IpxeScript)?;
            let body = artifacts::read_text(&app.settings, session.as_str(), "boot.ipxe")?
                .ok_or_else(|| Failure::not_found("脚本缺失"))?;
            Ok(ipxe(body))
        }
        "preseed.cfg" => once(
            &app,
            &authorize(&app, &token, Resource::Preseed)?,
            ServedDoc::Preseed,
            "preseed.cfg",
            &ip,
        ),
        "partman-early.sh" | "disk-select.sh" | "late.sh" => {
            text(&app, &authorize(&app, &token, Resource::Preseed)?, &rest)
        }
        "plan.json" => once(
            &app,
            &authorize(&app, &token, Resource::Plan)?,
            ServedDoc::Plan,
            "plan.json",
            &ip,
        ),
        "linux" | "initrd.gz" | "firmware.cpio.gz" | "agent" | "boot.iso" => {
            binary(&app, &token, &rest, &headers).await
        }
        other => match other
            .strip_prefix("repo-key/")
            .and_then(|name| name.strip_suffix(".asc"))
        {
            Some(index) if index.bytes().all(|byte| byte.is_ascii_digit()) => text(
                &app,
                &authorize(&app, &token, Resource::RepoKey)?,
                &format!("repo-key-{index}.asc"),
            ),
            Some(_) | None => Err(Failure::not_found("未知资源")),
        },
    }
}
