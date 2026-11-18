//! 到控制面内部 API（`/internal/v1/bootnet/*`）的 HTTP 客户端。
//!
//! 只支持明文 HTTP：内部 API 只映射到宿主机回环，令牌走 `Authorization: Bearer`。

use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use http::{Method, Request, StatusCode, header};
use http_body_util::{BodyExt as _, Full, Limited};
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::TokioExecutor;
use serde::Serialize;
use serde::de::DeserializeOwned;
use sha2::{Digest as _, Sha256};
use xpxe_domain::{DirectiveSnapshot, Heartbeat, HeartbeatRecord, Observation, Sha256Hex};

use crate::BootnetError;
use crate::settings::Settings;

/// 长轮询的总超时（服务端 25 s 后返回 304）。
const POLL_TIMEOUT: Duration = Duration::from_secs(35);
/// 普通请求超时。
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
/// 固件下载超时。
const FIRMWARE_TIMEOUT: Duration = Duration::from_secs(120);
/// JSON 响应上限。
const JSON_LIMIT: usize = 16 * 1024 * 1024;
/// 固件响应上限。
const FIRMWARE_LIMIT: usize = 64 * 1024 * 1024;

/// 控制面客户端。
#[derive(Debug, Clone)]
pub struct ControlClient {
    /// hyper 客户端（连接复用）。
    client: Client<HttpConnector, Full<Bytes>>,
    /// 基址，例如 `http://127.0.0.1:8081`。
    base: String,
    /// `Bearer <令牌>`。
    authorization: String,
}

/// 一次请求的超时与正文上限。
#[derive(Debug, Clone, Copy)]
struct Budget {
    /// 总超时。
    timeout: Duration,
    /// 正文上限。
    limit: usize,
}

impl Budget {
    /// 构造。
    const fn new(timeout: Duration, limit: usize) -> Self {
        Self { timeout, limit }
    }
}

/// 一次响应：状态码与正文。
struct Reply {
    /// 状态码。
    status: StatusCode,
    /// 正文。
    body: Bytes,
}

impl ControlClient {
    /// 由配置建立客户端。
    #[must_use]
    pub fn new(settings: &Settings) -> Self {
        let mut connector = HttpConnector::new();
        connector.set_connect_timeout(Some(Duration::from_secs(5)));
        connector.set_nodelay(true);
        Self {
            client: Client::builder(TokioExecutor::new()).build(connector),
            base: settings.control_url.clone(),
            authorization: format!("Bearer {}", settings.token.expose()),
        }
    }

    /// 发送请求并读取正文。
    ///
    /// # Errors
    ///
    /// 连接、超时或正文超限时返回 `control` 错误。
    async fn call(
        &self,
        method: Method,
        path: &str,
        body: Option<Vec<u8>>,
        budget: Budget,
    ) -> Result<Reply, BootnetError> {
        let Budget { timeout, limit } = budget;
        let uri = format!("{}/internal/v1/bootnet{path}", self.base);
        let mut builder = Request::builder()
            .method(method)
            .uri(&uri)
            .header(header::AUTHORIZATION, &self.authorization);
        if body.is_some() {
            builder = builder.header(header::CONTENT_TYPE, "application/json");
        }
        let request = builder
            .body(Full::new(Bytes::from(body.unwrap_or_default())))
            .map_err(|error| {
                BootnetError::new("control", format!("构造请求 {path} 失败：{error}"))
            })?;
        let exchange = async {
            let response = self
                .client
                .request(request)
                .await
                .map_err(|error| BootnetError::new("control", format!("{path}：{error}")))?;
            let status = response.status();
            let collected = Limited::new(response.into_body(), limit)
                .collect()
                .await
                .map_err(|error| BootnetError::new("control", format!("{path} 正文：{error}")))?;
            Ok(Reply {
                status,
                body: collected.to_bytes(),
            })
        };
        tokio::time::timeout(timeout, exchange)
            .await
            .map_err(|_elapsed| BootnetError::new("control", format!("{path} 超时")))?
    }

    /// 拉取指令快照：`rev` 为当前已应用版本；无变化（304）返回 `None`。
    ///
    /// # Errors
    ///
    /// 网络错误、非 200/304 或 JSON 非法时返回错误。
    pub async fn directives(
        &self,
        rev: Option<u64>,
    ) -> Result<Option<DirectiveSnapshot>, BootnetError> {
        let path = rev.map_or_else(
            || "/directives".to_owned(),
            |value| format!("/directives?rev={value}"),
        );
        let reply = self
            .call(
                Method::GET,
                &path,
                None,
                Budget::new(POLL_TIMEOUT, JSON_LIMIT),
            )
            .await?;
        match reply.status {
            StatusCode::NOT_MODIFIED => Ok(None),
            StatusCode::OK => decode(&reply.body).map(Some),
            other => Err(status_error(&path, other)),
        }
    }

    /// 下载固件并校验 SHA-256。
    ///
    /// # Errors
    ///
    /// 网络错误、非 200 或摘要不符时返回错误。
    pub async fn firmware(&self, digest: &Sha256Hex) -> Result<Arc<[u8]>, BootnetError> {
        let path = format!("/firmware/{}", digest.as_str());
        let reply = self
            .call(
                Method::GET,
                &path,
                None,
                Budget::new(FIRMWARE_TIMEOUT, FIRMWARE_LIMIT),
            )
            .await?;
        if reply.status != StatusCode::OK {
            return Err(status_error(&path, reply.status));
        }
        if !digest_matches(digest, &reply.body) {
            return Err(BootnetError::new(
                "firmware",
                format!("固件 {digest} 摘要不符，丢弃"),
            ));
        }
        Ok(Arc::from(reply.body.as_ref()))
    }

    /// 批量上报观测。
    ///
    /// # Errors
    ///
    /// 网络错误或非 2xx 时返回错误。
    pub async fn post_observations(&self, list: &[Observation]) -> Result<(), BootnetError> {
        self.post("/observations", &list).await
    }

    /// 上报心跳。
    ///
    /// # Errors
    ///
    /// 网络错误或非 2xx 时返回错误。
    pub async fn post_heartbeat(&self, beat: &Heartbeat) -> Result<(), BootnetError> {
        self.post("/heartbeat", beat).await
    }

    /// 读取控制面记录的最近一次心跳（`healthcheck` 用）。
    ///
    /// # Errors
    ///
    /// 网络错误、非 200 或 JSON 非法时返回错误。
    pub async fn heartbeat(&self) -> Result<HeartbeatRecord, BootnetError> {
        let reply = self
            .call(
                Method::GET,
                "/heartbeat",
                None,
                Budget::new(REQUEST_TIMEOUT, JSON_LIMIT),
            )
            .await?;
        if reply.status != StatusCode::OK {
            return Err(status_error("/heartbeat", reply.status));
        }
        decode(&reply.body)
    }

    /// POST 一个 JSON 正文。
    ///
    /// # Errors
    ///
    /// 序列化、网络错误或非 2xx 时返回错误。
    async fn post<T: Serialize + Sync + ?Sized>(
        &self,
        path: &str,
        value: &T,
    ) -> Result<(), BootnetError> {
        let body = serde_json::to_vec(value)
            .map_err(|error| BootnetError::new("control", format!("序列化 {path}：{error}")))?;
        let reply = self
            .call(
                Method::POST,
                path,
                Some(body),
                Budget::new(REQUEST_TIMEOUT, JSON_LIMIT),
            )
            .await?;
        if reply.status.is_success() {
            Ok(())
        } else {
            Err(status_error(path, reply.status))
        }
    }
}

/// 解析 JSON 正文。
///
/// # Errors
///
/// JSON 非法时返回错误。
fn decode<T: DeserializeOwned>(body: &[u8]) -> Result<T, BootnetError> {
    serde_json::from_slice(body)
        .map_err(|error| BootnetError::new("control", format!("响应 JSON 非法：{error}")))
}

/// 非预期状态码。
fn status_error(path: &str, status: StatusCode) -> BootnetError {
    BootnetError::new("control", format!("{path} 返回 {status}"))
}

/// 内容的 SHA-256 是否等于 `digest`。
#[must_use]
pub fn digest_matches(digest: &Sha256Hex, bytes: &[u8]) -> bool {
    let actual: [u8; 32] = Sha256::digest(bytes).into();
    Sha256Hex::from_digest(&actual) == *digest
}
