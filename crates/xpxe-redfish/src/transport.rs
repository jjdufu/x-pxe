//! HTTPS 传输：hyper + rustls（自定义证书校验器），JSON 请求/响应，证书变化识别。

use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine as _;
use bytes::Bytes;
use http::{Method, Request, StatusCode, header};
use http_body_util::{BodyExt as _, Full, Limited};
use hyper_rustls::HttpsConnector;
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::TokioExecutor;
use serde_json::Value;

use crate::pin::{BmcVerifier, Fingerprint, SeenSlot};
use crate::settings::{Credentials, Endpoint};
use crate::{RedfishError, RedfishResult};

/// 响应体上限。
const BODY_LIMIT: usize = 4 * 1024 * 1024;

/// 认证方式。`Debug` 不输出凭据。
#[derive(Clone, PartialEq, Eq)]
pub(crate) enum Auth {
    /// 不带认证（只用于 `GET /redfish/v1` 与创建会话）。
    Anonymous,
    /// `Authorization: Basic …`。
    Basic(String),
    /// `X-Auth-Token`。
    Token(String),
}

impl Auth {
    /// 由凭据构造 Basic 认证。
    pub(crate) fn basic(credentials: &Credentials) -> Self {
        let raw = format!("{}:{}", credentials.username(), credentials.password());
        Self::Basic(base64::engine::general_purpose::STANDARD.encode(raw))
    }
}

impl std::fmt::Debug for Auth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Anonymous => "Anonymous",
            Self::Basic(_) => "Basic(<redacted>)",
            Self::Token(_) => "Token(<redacted>)",
        })
    }
}

/// 一次响应。
#[derive(Debug, Clone)]
pub(crate) struct Reply {
    /// 状态码。
    pub(crate) status: StatusCode,
    /// `Location` 头。
    pub(crate) location: Option<String>,
    /// `X-Auth-Token` 头。
    pub(crate) token: Option<String>,
    /// JSON 正文（空正文或非 JSON 时为 `Null`）。
    pub(crate) body: Value,
}

impl Reply {
    /// 2xx 时返回自身，否则转为错误（401 → `unauthorized`，其余 → `http_status` 并附 `ExtendedInfo`）。
    ///
    /// # Errors
    ///
    /// 非 2xx 时返回错误。
    pub(crate) fn success(self, what: &str) -> RedfishResult<Self> {
        if self.status.is_success() {
            return Ok(self);
        }
        let detail = crate::wire::extended_info(&self.body);
        let code = if self.status == StatusCode::UNAUTHORIZED {
            "unauthorized"
        } else {
            "http_status"
        };
        Err(RedfishError::new(
            code,
            format!("{what}：HTTP {}{}", self.status.as_u16(), suffix(&detail)),
        ))
    }
}

/// 非空时加前缀“：”。
fn suffix(detail: &str) -> String {
    if detail.is_empty() {
        String::new()
    } else {
        format!("：{detail}")
    }
}

/// HTTPS 传输。
#[derive(Debug, Clone)]
pub(crate) struct Transport {
    /// hyper 客户端。
    client: Client<HttpsConnector<HttpConnector>, Full<Bytes>>,
    /// BMC 地址。
    base: Endpoint,
    /// 固定指纹（探测模式为 `None`）。
    pin: Option<Fingerprint>,
    /// 最近握手看到的证书。
    seen: SeenSlot,
    /// 单次请求超时。
    timeout: Duration,
}

impl Transport {
    /// 构造传输；`pin` 为 `None` 时为探测模式。
    ///
    /// # Errors
    ///
    /// TLS 配置无法建立时返回 `transport`。
    pub(crate) fn new(
        base: &Endpoint,
        pin: Option<Fingerprint>,
        timeout: Duration,
    ) -> RedfishResult<Self> {
        let seen: SeenSlot = Arc::new(Mutex::new(None));
        let verifier = Arc::new(BmcVerifier::new(pin, Arc::clone(&seen)));
        let config = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .map_err(|error| RedfishError::new("transport", format!("TLS 配置失败：{error}")))?
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();
        let connector = hyper_rustls::HttpsConnectorBuilder::new()
            .with_tls_config(config)
            .https_only()
            .enable_http1()
            .build();
        Ok(Self {
            client: Client::builder(TokioExecutor::new()).build(connector),
            base: base.clone(),
            pin,
            seen,
            timeout,
        })
    }

    /// 最近握手看到的终端证书 DER。
    pub(crate) fn seen_der(&self) -> Option<Vec<u8>> {
        self.seen.lock().ok()?.clone()
    }

    /// 绝对 URL：相对路径拼到 BMC 地址；绝对 URL 必须仍指向同一 BMC（不把凭据发往别处）。
    ///
    /// # Errors
    ///
    /// 路径不合法或指向其它主机时返回 `protocol`。
    fn url(&self, path: &str) -> RedfishResult<String> {
        let base = self.base.as_str();
        if path.starts_with('/') {
            return Ok(format!("{base}{path}"));
        }
        if path
            .strip_prefix(base)
            .is_some_and(|rest| rest.starts_with('/'))
        {
            return Ok(path.to_owned());
        }
        Err(RedfishError::new(
            "protocol",
            format!("BMC 返回了不属于自身的地址：{path}"),
        ))
    }

    /// 传输层失败：固定模式下若看到的证书指纹与固定值不同，报告 `cert_changed`。
    fn failure(&self, detail: &str) -> RedfishError {
        let actual = self.seen_der().map(|der| Fingerprint::of_der(&der));
        match (self.pin, actual) {
            (Some(expected), Some(seen)) if expected != seen => RedfishError::new(
                "cert_changed",
                format!("BMC 证书已变化：固定指纹 {expected}，实际 {seen}；需重新探测并确认"),
            ),
            _other => RedfishError::new("transport", format!("连接 BMC 失败：{detail}")),
        }
    }

    /// 发送请求并读取 JSON 响应。
    ///
    /// # Errors
    ///
    /// 地址不合法、连接/TLS 失败、证书变化、超时或响应过大时返回错误（HTTP 状态码不在此判断）。
    pub(crate) async fn send(
        &self,
        method: Method,
        path: &str,
        auth: &Auth,
        body: Option<&Value>,
    ) -> RedfishResult<Reply> {
        let url = self.url(path)?;
        let mut builder = Request::builder()
            .method(method)
            .uri(&url)
            .header(header::ACCEPT, "application/json")
            .header("OData-Version", "4.0")
            .header(header::USER_AGENT, "x-pxe-redfish");
        builder = match auth {
            Auth::Anonymous => builder,
            Auth::Basic(encoded) => {
                builder.header(header::AUTHORIZATION, format!("Basic {encoded}"))
            }
            Auth::Token(token) => builder.header("X-Auth-Token", token),
        };
        if body.is_some() {
            builder = builder.header(header::CONTENT_TYPE, "application/json");
        }
        let payload = body.map_or_else(Bytes::new, |json| Bytes::from(json.to_string()));
        let request = builder
            .body(Full::new(payload))
            .map_err(|error| RedfishError::new("protocol", format!("请求无法构造：{error}")))?;
        let response = tokio::time::timeout(self.timeout, self.client.request(request))
            .await
            .map_err(|_elapsed| RedfishError::new("timeout", format!("{url}：等待响应超时")))?
            .map_err(|error| self.failure(&format!("{error:?}")))?;
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = tokio::time::timeout(
            self.timeout,
            Limited::new(response.into_body(), BODY_LIMIT).collect(),
        )
        .await
        .map_err(|_elapsed| RedfishError::new("timeout", format!("{url}：读取响应超时")))?
        .map_err(|error| RedfishError::new("transport", format!("{url}：读取响应失败：{error}")))?
        .to_bytes();
        let header_text = |name: &str| {
            headers
                .get(name)
                .and_then(|value| value.to_str().ok())
                .map(ToOwned::to_owned)
        };
        Ok(Reply {
            status,
            location: header_text("location"),
            token: header_text("x-auth-token"),
            body: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        })
    }
}
