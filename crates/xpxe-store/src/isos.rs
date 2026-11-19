//! 手动引导用 ISO 元数据、抽取产物缓存与进行中的分块上传（Step 14）。

use rusqlite::Row;
use serde::Serialize;
use serde_json::Value;
use xpxe_domain::Timestamp;

use crate::sqlval::{exec, parse_opt_ts, parse_ts, query_opt, query_vec, to_i64, to_u64, ts};
use crate::{Store, StoreError, StoreResult};

/// 新 ISO 的元数据。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewIso {
    /// 内容 SHA-256。
    pub sha256: String,
    /// 字节数。
    pub size: u64,
    /// 管理员填写的标签。
    pub label: String,
    /// 卷标。
    pub volume_id: String,
    /// 是否 hybrid（含 MBR/GPT）。
    pub hybrid: bool,
    /// 识别出的发行版族。
    pub detected_family: String,
    /// 完整识别结果（JSON）。
    pub detection: Value,
    /// 上传者。
    pub uploaded_by: String,
}

/// ISO 记录。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IsoRecord {
    /// ID。
    pub id: i64,
    /// SHA-256。
    pub sha256: String,
    /// 字节数。
    pub size: u64,
    /// 标签。
    pub label: String,
    /// 卷标。
    pub volume_id: String,
    /// 是否 hybrid。
    pub hybrid: bool,
    /// 发行版族。
    pub detected_family: String,
    /// 识别结果。
    pub detection: Value,
    /// 上传者。
    pub uploaded_by: String,
    /// 上传时间。
    pub uploaded_at: Timestamp,
    /// 删除时间。
    pub deleted_at: Option<Timestamp>,
}

/// 抽取产物键。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExtractKey {
    /// 源 ISO。
    pub iso_sha256: String,
    /// `kernel` / `initrd` / `initrd_with_iso`。
    pub kind: String,
    /// ISO 内路径。
    pub source_path: String,
    /// 嵌入 ISO 时在 initrd 中的文件名（否则为空）。
    pub embed_name: String,
}

/// 抽取产物。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExtractRecord {
    /// 键。
    #[serde(flatten)]
    pub key: ExtractKey,
    /// 产物 SHA-256。
    pub sha256: String,
    /// 字节数。
    pub size: u64,
}

/// 进行中的上传。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UploadRecord {
    /// 上传 ID。
    pub id: String,
    /// 标签。
    pub label: String,
    /// 声明的总字节数。
    pub size: u64,
    /// 期望的 SHA-256。
    pub expected_sha256: Option<String>,
    /// 创建者。
    pub created_by: String,
    /// 创建时间。
    pub created_at: Timestamp,
    /// 最近一次收到分块的时间。
    pub updated_at: Timestamp,
}

/// ISO 句柄。
#[derive(Debug, Clone, Copy)]
pub struct Isos<'store> {
    /// 所属存储。
    store: &'store Store,
}

/// 校验十六进制摘要。
///
/// # Errors
///
/// 不是 64 位十六进制时返回错误。
fn check_sha(sha: &str) -> StoreResult<String> {
    if sha.len() == 64 && sha.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(sha.to_ascii_lowercase())
    } else {
        Err(StoreError::Invalid(
            "sha256 必须为 64 位十六进制".to_owned(),
        ))
    }
}

impl<'store> Isos<'store> {
    /// 构造句柄。
    pub(crate) const fn new(store: &'store Store) -> Self {
        Self { store }
    }

    /// 登记 ISO；同一内容已存在（未删除）时返回已有 ID。
    ///
    /// # Errors
    ///
    /// 摘要不合法或写入失败时返回错误。
    pub fn insert(self, iso: &NewIso, now: Timestamp) -> StoreResult<i64> {
        let sha = check_sha(&iso.sha256)?;
        if let Some(existing) = self.live(&sha)? {
            return Ok(existing.id);
        }
        self.store.write(|tx| {
            exec(
                tx,
                "INSERT INTO iso_images (sha256, size, label, volume_id, hybrid, detected_family, detection,
                   uploaded_by, uploaded_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                rusqlite::params![
                    sha,
                    to_i64(iso.size)?,
                    iso.label,
                    iso.volume_id,
                    iso.hybrid,
                    iso.detected_family,
                    serde_json::to_string(&iso.detection)?,
                    iso.uploaded_by,
                    ts(now)?,
                ],
            )?;
            Ok(tx.last_insert_rowid())
        })
    }

    /// 未删除的 ISO（新到旧）。
    ///
    /// # Errors
    ///
    /// 查询失败时返回错误。
    pub fn list(self) -> StoreResult<Vec<IsoRecord>> {
        self.store.read(|conn| {
            query_vec(
                conn,
                &format!("{SELECT_ISO} WHERE deleted_at IS NULL ORDER BY id DESC"),
                [],
                iso_row,
            )
        })
    }

    /// 按摘要取未删除的 ISO。
    ///
    /// # Errors
    ///
    /// 查询失败时返回错误。
    pub fn live(self, sha256: &str) -> StoreResult<Option<IsoRecord>> {
        let sha = sha256.to_ascii_lowercase();
        self.store.read(|conn| {
            query_opt(
                conn,
                &format!("{SELECT_ISO} WHERE sha256 = ?1 AND deleted_at IS NULL"),
                [sha],
                iso_row,
            )
        })
    }

    /// 标记删除并清掉抽取记录，返回需删除的产物摘要（调用方删除文件）。
    ///
    /// # Errors
    ///
    /// 不存在或写入失败时返回错误。
    pub fn delete(self, sha256: &str, now: Timestamp) -> StoreResult<Vec<String>> {
        let sha = check_sha(sha256)?;
        let extracts = self.extracts(&sha)?;
        let changed = self.store.write(|tx| {
            let changed = tx.execute(
                "UPDATE iso_images SET deleted_at = ?2 WHERE sha256 = ?1 AND deleted_at IS NULL",
                rusqlite::params![sha, ts(now)?],
            )?;
            exec(tx, "DELETE FROM iso_extracts WHERE iso_sha256 = ?1", [&sha])?;
            Ok(changed)
        })?;
        if changed == 1 {
            Ok(extracts.into_iter().map(|item| item.sha256).collect())
        } else {
            Err(StoreError::NotFound("ISO"))
        }
    }

    /// 某 ISO 的全部抽取产物。
    ///
    /// # Errors
    ///
    /// 查询失败时返回错误。
    pub fn extracts(self, iso_sha256: &str) -> StoreResult<Vec<ExtractRecord>> {
        let sha = iso_sha256.to_ascii_lowercase();
        self.store.read(|conn| {
            query_vec(
                conn,
                &format!("{SELECT_EXTRACT} WHERE iso_sha256 = ?1 ORDER BY kind, source_path"),
                [sha],
                extract_row,
            )
        })
    }

    /// 按键取抽取产物。
    ///
    /// # Errors
    ///
    /// 查询失败时返回错误。
    pub fn extract(self, key: &ExtractKey) -> StoreResult<Option<ExtractRecord>> {
        self.store.read(|conn| {
            query_opt(
                conn,
                &format!(
                    "{SELECT_EXTRACT} WHERE iso_sha256 = ?1 AND kind = ?2 AND source_path = ?3 AND embed_name = ?4"
                ),
                rusqlite::params![key.iso_sha256, key.kind, key.source_path, key.embed_name],
                extract_row,
            )
        })
    }

    /// 产物摘要是否属于某个未删除 ISO 的抽取结果（`/m/x/{sha}` 只服务这些）。
    ///
    /// # Errors
    ///
    /// 查询失败时返回错误。
    pub fn extract_by_sha(self, sha256: &str) -> StoreResult<Option<ExtractRecord>> {
        let sha = sha256.to_ascii_lowercase();
        self.store.read(|conn| {
            query_opt(
                conn,
                &format!("{SELECT_EXTRACT} WHERE sha256 = ?1 LIMIT 1"),
                [sha],
                extract_row,
            )
        })
    }

    /// 记录抽取产物（同键覆盖）。
    ///
    /// # Errors
    ///
    /// 摘要不合法或写入失败时返回错误。
    pub fn put_extract(self, record: &ExtractRecord, now: Timestamp) -> StoreResult<()> {
        let sha = check_sha(&record.sha256)?;
        self.store.write(|tx| {
            exec(
                tx,
                "INSERT OR REPLACE INTO iso_extracts (iso_sha256, kind, source_path, embed_name, sha256, size, created_at)
                   VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    record.key.iso_sha256,
                    record.key.kind,
                    record.key.source_path,
                    record.key.embed_name,
                    sha,
                    to_i64(record.size)?,
                    ts(now)?,
                ],
            )
        })
    }

    /// 开始一次上传。
    ///
    /// # Errors
    ///
    /// ID 冲突或写入失败时返回错误。
    pub fn begin_upload(self, upload: &UploadRecord) -> StoreResult<()> {
        let expected = upload
            .expected_sha256
            .as_deref()
            .map(check_sha)
            .transpose()?;
        self.store.write(|tx| {
            exec(
                tx,
                "INSERT INTO iso_uploads (id, label, size, expected_sha256, created_by, created_at, updated_at)
                   VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    upload.id,
                    upload.label,
                    to_i64(upload.size)?,
                    expected,
                    upload.created_by,
                    ts(upload.created_at)?,
                    ts(upload.updated_at)?,
                ],
            )
        })
    }

    /// 读取上传。
    ///
    /// # Errors
    ///
    /// 不存在时返回错误。
    pub fn upload(self, upload_id: &str) -> StoreResult<UploadRecord> {
        self.store.read(|conn| {
            query_opt(
                conn,
                &format!("{SELECT_UPLOAD} WHERE id = ?1"),
                [upload_id],
                upload_row,
            )?
            .ok_or(StoreError::NotFound("上传"))
        })
    }

    /// 全部进行中的上传。
    ///
    /// # Errors
    ///
    /// 查询失败时返回错误。
    pub fn uploads(self) -> StoreResult<Vec<UploadRecord>> {
        self.store.read(|conn| {
            query_vec(
                conn,
                &format!("{SELECT_UPLOAD} ORDER BY created_at"),
                [],
                upload_row,
            )
        })
    }

    /// 收到分块后刷新时间。
    ///
    /// # Errors
    ///
    /// 写入失败时返回错误。
    pub fn touch_upload(self, upload_id: &str, now: Timestamp) -> StoreResult<()> {
        self.store.write(|tx| {
            exec(
                tx,
                "UPDATE iso_uploads SET updated_at = ?2 WHERE id = ?1",
                rusqlite::params![upload_id, ts(now)?],
            )
        })
    }

    /// 结束（完成或放弃）上传。
    ///
    /// # Errors
    ///
    /// 写入失败时返回错误。
    pub fn end_upload(self, upload_id: &str) -> StoreResult<()> {
        self.store
            .write(|tx| exec(tx, "DELETE FROM iso_uploads WHERE id = ?1", [upload_id]))
    }
}

/// ISO 查询列。
const SELECT_ISO: &str =
    "SELECT id, sha256, size, label, volume_id, hybrid, detected_family, detection,
  uploaded_by, uploaded_at, deleted_at FROM iso_images";
/// 抽取查询列。
const SELECT_EXTRACT: &str =
    "SELECT iso_sha256, kind, source_path, embed_name, sha256, size FROM iso_extracts";
/// 上传查询列。
const SELECT_UPLOAD: &str =
    "SELECT id, label, size, expected_sha256, created_by, created_at, updated_at FROM iso_uploads";

/// 行 → ISO。
///
/// # Errors
///
/// 列内容不合法时返回错误。
fn iso_row(row: &Row<'_>) -> StoreResult<IsoRecord> {
    Ok(IsoRecord {
        id: row.get(0)?,
        sha256: row.get(1)?,
        size: to_u64(row.get(2)?)?,
        label: row.get(3)?,
        volume_id: row.get(4)?,
        hybrid: row.get(5)?,
        detected_family: row.get(6)?,
        detection: serde_json::from_str(&row.get::<_, String>(7)?)?,
        uploaded_by: row.get(8)?,
        uploaded_at: parse_ts(&row.get::<_, String>(9)?)?,
        deleted_at: parse_opt_ts(row.get::<_, Option<String>>(10)?.as_deref())?,
    })
}

/// 行 → 抽取产物。
///
/// # Errors
///
/// 列内容不合法时返回错误。
fn extract_row(row: &Row<'_>) -> StoreResult<ExtractRecord> {
    Ok(ExtractRecord {
        key: ExtractKey {
            iso_sha256: row.get(0)?,
            kind: row.get(1)?,
            source_path: row.get(2)?,
            embed_name: row.get(3)?,
        },
        sha256: row.get(4)?,
        size: to_u64(row.get(5)?)?,
    })
}

/// 行 → 上传。
///
/// # Errors
///
/// 列内容不合法时返回错误。
fn upload_row(row: &Row<'_>) -> StoreResult<UploadRecord> {
    Ok(UploadRecord {
        id: row.get(0)?,
        label: row.get(1)?,
        size: to_u64(row.get(2)?)?,
        expected_sha256: row.get(3)?,
        created_by: row.get(4)?,
        created_at: parse_ts(&row.get::<_, String>(5)?)?,
        updated_at: parse_ts(&row.get::<_, String>(6)?)?,
    })
}
