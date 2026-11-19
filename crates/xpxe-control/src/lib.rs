//! x-pxe 控制面：管理端（`/api/v1`、`WebUI`）、引导端（iPXE/preseed/事件）与内部 API（bootnet）。
//!
//! 三个监听各自一个 Router，互不挂载对方路由。状态机副作用经发件箱由后台 worker 执行，
//! 超时由单个调度任务按最近截止时间投递。时钟通过 [`clock::Clock`] 注入，便于端到端测试拨动。
#![doc(test(attr(deny(warnings), forbid(unsafe_code))))]

pub mod app;
pub mod cli;
pub mod clock;
pub mod server;
pub mod settings;

mod admin;
mod arm;
mod artifacts;
mod assets;
mod auth;
mod bmc;
mod boot;
mod configs;
mod directives;
mod effects;
mod hub;
mod ingest;
mod internal;
mod manual;
mod nodeops;
mod nodes;
mod precheck;
mod preview;
mod served;
mod timers;
mod tls;
mod transition;
mod views;
mod web;

use std::fmt::{Display, Formatter};

use axum::Json;
use axum::response::{IntoResponse, Response};
use http::StatusCode;
use serde::Serialize;
use serde_json::json;
use xpxe_domain::{ConfigError, FieldError, TransitionError};
use xpxe_store::StoreError;

/// 字段级错误（`path` 为 JSON Pointer）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FieldIssue {
    /// JSON Pointer。
    pub path: String,
    /// 稳定错误码。
    pub code: String,
    /// 人类可读说明。
    pub message: String,
}

impl FieldIssue {
    /// 构造。
    #[must_use]
    pub fn new(path: &str, code: &str, message: &str) -> Self {
        Self {
            path: path.to_owned(),
            code: code.to_owned(),
            message: message.to_owned(),
        }
    }
}

impl From<&FieldError> for FieldIssue {
    fn from(value: &FieldError) -> Self {
        Self {
            path: value.path.clone(),
            code: value.code.clone(),
            message: value.message.clone(),
        }
    }
}

/// 失败详情。
#[derive(Debug, Clone, PartialEq, Eq)]
struct FailureBody {
    /// HTTP 状态。
    status: StatusCode,
    /// 稳定错误码（大写蛇形）。
    code: String,
    /// 说明。
    message: String,
    /// 字段错误。
    fields: Vec<FieldIssue>,
}

/// 控制面统一错误：HTTP 状态 + `{error:{code,message,fields}}`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure(Box<FailureBody>);

/// 控制面结果。
pub type Outcome<T> = Result<T, Failure>;

impl Failure {
    /// 构造。
    #[must_use]
    pub fn new<M: Into<String>>(status: StatusCode, code: &str, message: M) -> Self {
        Self(Box::new(FailureBody {
            status,
            code: code.to_owned(),
            message: message.into(),
            fields: Vec::new(),
        }))
    }

    /// 附带字段错误。
    #[must_use]
    pub fn with_fields(mut self, fields: Vec<FieldIssue>) -> Self {
        self.0.fields = fields;
        self
    }

    /// 400。
    #[must_use]
    pub fn bad_request<M: Into<String>>(code: &str, message: M) -> Self {
        Self::new(StatusCode::BAD_REQUEST, code, message)
    }

    /// 404。
    #[must_use]
    pub fn not_found<M: Into<String>>(message: M) -> Self {
        Self::new(StatusCode::NOT_FOUND, "NOT_FOUND", message)
    }

    /// 403。
    #[must_use]
    pub fn forbidden<M: Into<String>>(code: &str, message: M) -> Self {
        Self::new(StatusCode::FORBIDDEN, code, message)
    }

    /// 409。
    #[must_use]
    pub fn conflict<M: Into<String>>(code: &str, message: M) -> Self {
        Self::new(StatusCode::CONFLICT, code, message)
    }

    /// 500。
    #[must_use]
    pub fn internal<M: Into<String>>(message: M) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL", message)
    }

    /// 422 + 字段错误。
    #[must_use]
    pub fn invalid(fields: Vec<FieldIssue>) -> Self {
        Self::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "VALIDATION_FAILED",
            "校验失败",
        )
        .with_fields(fields)
    }

    /// HTTP 状态。
    #[must_use]
    pub fn status(&self) -> StatusCode {
        self.0.status
    }

    /// 错误码。
    #[must_use]
    pub fn code(&self) -> &str {
        &self.0.code
    }

    /// 说明。
    #[must_use]
    pub fn message(&self) -> &str {
        &self.0.message
    }

    /// 字段错误。
    #[must_use]
    pub fn fields(&self) -> &[FieldIssue] {
        &self.0.fields
    }
}

impl Display for Failure {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}：{}", self.0.code, self.0.message)
    }
}

impl std::error::Error for Failure {}

impl IntoResponse for Failure {
    fn into_response(self) -> Response {
        let body = json!({
            "error": { "code": self.0.code, "message": self.0.message, "fields": self.0.fields }
        });
        (self.0.status, Json(body)).into_response()
    }
}

impl From<StoreError> for Failure {
    fn from(value: StoreError) -> Self {
        match value {
            StoreError::NotFound(what) => Self::not_found(format!("{what}不存在")),
            StoreError::Conflict { current_head } => Self::conflict(
                "VERSION_CONFLICT",
                format!("配置已被修改，当前头版本 {current_head}"),
            ),
            StoreError::StateChanged { current } => {
                Self::conflict("STATE_CHANGED", format!("节点状态已变为 {current}"))
            }
            StoreError::Config(error) => Self::from(*error),
            StoreError::Invalid(message) => Self::bad_request("INVALID", message),
            StoreError::AlreadyServed => {
                Self::new(StatusCode::GONE, "ALREADY_SERVED", "一次性文档已下发")
            }
            StoreError::Throttled { retry_after } => Self::new(
                StatusCode::TOO_MANY_REQUESTS,
                "LOGIN_THROTTLED",
                format!("登录失败过多，{retry_after} 秒后再试"),
            ),
            StoreError::InvalidCredentials => Self::new(
                StatusCode::UNAUTHORIZED,
                "INVALID_CREDENTIALS",
                "用户名或密码错误",
            ),
            other @ (StoreError::Sqlite(_)
            | StoreError::Io(_)
            | StoreError::MasterKey(_)
            | StoreError::KeyMismatch
            | StoreError::Crypto
            | StoreError::Poisoned
            | StoreError::Corrupt(_)) => Self::internal(other.to_string()),
        }
    }
}

impl From<ConfigError> for Failure {
    fn from(value: ConfigError) -> Self {
        match value {
            ConfigError::Invalid(error) => {
                Self::invalid(error.errors().iter().map(FieldIssue::from).collect())
            }
            other @ (ConfigError::UnsupportedSchema { .. } | ConfigError::Json(_)) => {
                Self::bad_request("BAD_DOCUMENT", other.to_string())
            }
        }
    }
}

impl From<TransitionError> for Failure {
    fn from(value: TransitionError) -> Self {
        let code = match value {
            TransitionError::NotAllowed { .. } => "TRANSITION_NOT_ALLOWED",
            TransitionError::StaleSession => "STALE_SESSION",
            TransitionError::StaleDeadline => "STALE_DEADLINE",
            TransitionError::BadConfirmation => "BAD_CONFIRMATION",
            TransitionError::MissingSession => "MISSING_SESSION",
        };
        Self::conflict(code, value.to_string())
    }
}

/// 把任意错误包装为 500。
pub(crate) fn internal<E: Display>(error: E) -> Failure {
    Failure::internal(error.to_string())
}

#[cfg(test)]
mod tests;
