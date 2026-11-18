//! 测试公共件。

use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use xpxe_domain::versioning::{field_error, migrate_current};
use xpxe_domain::{ConfigDocument, ConfigError, ConfigKind, FieldError, Timestamp};
use xpxe_store::{Authorship, MasterKey, Store, StoreResult};

/// 测试基准时间（2026-11-18 前后）。
pub(crate) const T0: u64 = 1_795_000_000;

/// 基准时间之后 `secs` 秒。
#[must_use]
pub(crate) const fn at(secs: u64) -> Timestamp {
    Timestamp::from_unix_seconds(T0.saturating_add(secs))
}

/// 固定主密钥 A。
#[must_use]
pub(crate) fn key_a() -> MasterKey {
    MasterKey::from_bytes([7; 32])
}

/// 固定主密钥 B。
#[must_use]
pub(crate) fn key_b() -> MasterKey {
    MasterKey::from_bytes([9; 32])
}

/// 用主密钥 A 打开。
///
/// # Errors
///
/// 打开失败时返回错误。
pub(crate) fn open(dir: &Path) -> StoreResult<Store> {
    Store::open(dir, key_a())
}

/// 系统作者。
#[must_use]
pub(crate) fn by(comment: &str, secs: u64) -> Authorship {
    Authorship {
        author_id: None,
        comment: comment.to_owned(),
        at: at(secs),
    }
}

/// 名称不能为空的测试文档，用 `bmc` 种类。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Picky {
    /// 名称。
    pub name: String,
    /// 数值。
    pub value: u32,
}

impl ConfigDocument for Picky {
    const KIND: ConfigKind = ConfigKind::Bmc;
    const SCHEMA: u32 = 1;

    fn validate(&self) -> Result<(), Vec<FieldError>> {
        if self.name.is_empty() {
            Err(vec![field_error("/name", "empty", "名称不能为空")])
        } else {
            Ok(())
        }
    }

    fn migrate(from_schema: u32, value: Value) -> Result<Self, ConfigError> {
        migrate_current(from_schema, value)
    }
}

/// 构造测试文档。
#[must_use]
pub(crate) fn picky(name: &str, value: u32) -> Picky {
    Picky {
        name: name.to_owned(),
        value,
    }
}
