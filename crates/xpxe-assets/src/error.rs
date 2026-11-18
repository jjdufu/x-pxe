//! 资产校验错误。

use std::fmt::{Display, Formatter};

/// 资产校验错误：稳定错误码 + 中文说明。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetsError {
    /// 稳定错误码。
    code: &'static str,
    /// 说明。
    message: String,
}

impl AssetsError {
    /// 构造错误。
    #[must_use]
    pub const fn new(code: &'static str, message: String) -> Self {
        Self { code, message }
    }

    /// 错误码。
    #[must_use]
    pub const fn code(&self) -> &'static str {
        self.code
    }

    /// 说明。
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl Display for AssetsError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for AssetsError {}

/// 以静态说明构造错误。
pub(crate) fn fail(code: &'static str, message: &str) -> AssetsError {
    AssetsError::new(code, message.to_owned())
}

/// 校验结果。
pub type AssetsResult<T> = Result<T, AssetsError>;
