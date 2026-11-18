//! 资产元数据（文件本体存于 `/data/assets/<sha256>`）。

use std::str::FromStr;

use rusqlite::Row;
use serde::Serialize;
use serde_json::Value;
use xpxe_domain::Timestamp;

use crate::sqlval::{exec, parse_ts, query_opt, query_vec, to_i64, to_u64, ts};
use crate::{Store, StoreError, StoreResult};

/// 资产来源。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetSource {
    /// 官方源。
    Official,
    /// 指定 URL。
    Url,
    /// 上传。
    Upload,
}

impl AssetSource {
    /// 存储名称。
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Official => "official",
            Self::Url => "url",
            Self::Upload => "upload",
        }
    }
}

impl FromStr for AssetSource {
    type Err = StoreError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "official" => Ok(Self::Official),
            "url" => Ok(Self::Url),
            "upload" => Ok(Self::Upload),
            other => Err(StoreError::Corrupt(format!("未知资产来源 {other}"))),
        }
    }
}

/// 资产阶段。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetStage {
    /// 已落盘待校验。
    Staged,
    /// 校验通过。
    Verified,
    /// 金丝雀通过。
    CanaryPassed,
    /// 已退役。
    Retired,
}

impl AssetStage {
    /// 存储名称。
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Staged => "staged",
            Self::Verified => "verified",
            Self::CanaryPassed => "canary_passed",
            Self::Retired => "retired",
        }
    }
}

impl FromStr for AssetStage {
    type Err = StoreError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "staged" => Ok(Self::Staged),
            "verified" => Ok(Self::Verified),
            "canary_passed" => Ok(Self::CanaryPassed),
            "retired" => Ok(Self::Retired),
            other => Err(StoreError::Corrupt(format!("未知资产阶段 {other}"))),
        }
    }
}

/// 新资产元数据。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewAsset {
    /// 种类（`kernel`、`initrd`、`ipxe`、`agent` …）。
    pub kind: String,
    /// 标签。
    pub label: String,
    /// 来源。
    pub source: AssetSource,
    /// 来源 URL。
    pub source_url: Option<String>,
    /// 内容 SHA-256（十六进制）。
    pub sha256: String,
    /// 字节数。
    pub size: u64,
    /// 签名校验结论。
    pub verify: Value,
    /// 内核 ABI。
    pub kernel_abi: Option<String>,
    /// 创建者。
    pub created_by: String,
}

/// 资产记录。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AssetRecord {
    /// ID。
    pub id: i64,
    /// 元数据。
    #[serde(flatten)]
    pub meta: AssetMeta,
    /// 阶段。
    pub stage: AssetStage,
    /// 创建时间。
    pub created_at: Timestamp,
}

/// 资产元数据（可序列化副本）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AssetMeta {
    /// 种类。
    pub kind: String,
    /// 标签。
    pub label: String,
    /// 来源。
    pub source: AssetSource,
    /// 来源 URL。
    pub source_url: Option<String>,
    /// SHA-256。
    pub sha256: String,
    /// 字节数。
    pub size: u64,
    /// 校验结论。
    pub verify: Value,
    /// 内核 ABI。
    pub kernel_abi: Option<String>,
    /// 创建者。
    pub created_by: String,
}

/// 资产句柄。
#[derive(Debug, Clone, Copy)]
pub struct Assets<'store> {
    /// 所属存储。
    store: &'store Store,
}

impl<'store> Assets<'store> {
    /// 构造句柄。
    pub(crate) const fn new(store: &'store Store) -> Self {
        Self { store }
    }

    /// 登记资产（阶段 `staged`）。
    ///
    /// # Errors
    ///
    /// 写入失败时返回错误。
    pub fn insert(self, asset: &NewAsset, now: Timestamp) -> StoreResult<i64> {
        if asset.sha256.len() != 64 || !asset.sha256.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(StoreError::Invalid(
                "sha256 必须为 64 位十六进制".to_owned(),
            ));
        }
        self.store.write(|tx| {
            exec(tx,
                "INSERT INTO assets (kind, label, source, source_url, sha256, size, verify, kernel_abi,
                   stage, created_by, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'staged', ?9, ?10)",
                rusqlite::params![
                    asset.kind,
                    asset.label,
                    asset.source.as_str(),
                    asset.source_url,
                    asset.sha256.to_ascii_lowercase(),
                    to_i64(asset.size)?,
                    serde_json::to_string(&asset.verify)?,
                    asset.kernel_abi,
                    asset.created_by,
                    ts(now)?,
                ],
            )?;
            Ok(tx.last_insert_rowid())
        })
    }

    /// 读取资产。
    ///
    /// # Errors
    ///
    /// 不存在时返回错误。
    pub fn get(self, asset_id: i64) -> StoreResult<AssetRecord> {
        self.store.read(|conn| {
            query_opt(
                conn,
                &format!("{SELECT_ASSET} WHERE id = ?1"),
                [asset_id],
                asset_row,
            )?
            .ok_or(StoreError::NotFound("资产"))
        })
    }

    /// 全部资产（新到旧）。
    ///
    /// # Errors
    ///
    /// 查询失败时返回错误。
    pub fn list(self) -> StoreResult<Vec<AssetRecord>> {
        self.store.read(|conn| {
            query_vec(
                conn,
                &format!("{SELECT_ASSET} ORDER BY id DESC"),
                [],
                asset_row,
            )
        })
    }

    /// 修改阶段与校验结论。
    ///
    /// # Errors
    ///
    /// 资产不存在或写入失败时返回错误。
    pub fn set_stage(
        self,
        asset_id: i64,
        stage: AssetStage,
        verify: Option<&Value>,
    ) -> StoreResult<()> {
        let verify_text = verify.map(serde_json::to_string).transpose()?;
        let changed = self.store.write(|tx| {
            Ok(tx.execute(
                "UPDATE assets SET stage = ?2, verify = coalesce(?3, verify) WHERE id = ?1",
                rusqlite::params![asset_id, stage.as_str(), verify_text],
            )?)
        })?;
        if changed == 1 {
            Ok(())
        } else {
            Err(StoreError::NotFound("资产"))
        }
    }
}

/// 资产查询的公共列。
const SELECT_ASSET: &str =
    "SELECT id, kind, label, source, source_url, sha256, size, verify, kernel_abi,
  stage, created_by, created_at FROM assets";

/// 行 → 资产记录。
///
/// # Errors
///
/// 列内容不合法时返回错误。
fn asset_row(row: &Row<'_>) -> StoreResult<AssetRecord> {
    Ok(AssetRecord {
        id: row.get(0)?,
        meta: AssetMeta {
            kind: row.get(1)?,
            label: row.get(2)?,
            source: row.get::<_, String>(3)?.parse()?,
            source_url: row.get(4)?,
            sha256: row.get(5)?,
            size: to_u64(row.get(6)?)?,
            verify: serde_json::from_str(&row.get::<_, String>(7)?)?,
            kernel_abi: row.get(8)?,
            created_by: row.get(10)?,
        },
        stage: row.get::<_, String>(9)?.parse()?,
        created_at: parse_ts(&row.get::<_, String>(11)?)?,
    })
}
