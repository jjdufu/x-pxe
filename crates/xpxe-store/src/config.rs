//! 配置对象与不可变版本链：相同内容不产生新版本，乐观并发，回滚 = 复制旧内容生成新版本。

use rusqlite::{Connection, Transaction};
use serde::Serialize;
use serde_json::Value;
use xpxe_domain::{Change, ConfigDocument, Timestamp, canonical_json, content_sha256_hex, diff};

use crate::sqlval::{exec, parse_ts, query_opt, query_vec, to_i64, to_u64, ts};
use crate::{Store, StoreError, StoreResult};

/// 指向某个对象某个版本的引用。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, serde::Deserialize)]
pub struct VersionRef {
    /// 对象 ID。
    pub object_id: i64,
    /// 版本号（从 1 开始）。
    pub version: u64,
    /// 规范化内容的 SHA-256（十六进制）。
    pub content_sha256: String,
}

/// 一次写入的作者、说明与时间。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Authorship {
    /// 作者用户 ID；系统写入时为空。
    pub author_id: Option<i64>,
    /// 变更说明。
    pub comment: String,
    /// 写入时间。
    pub at: Timestamp,
}

/// 版本元数据（不含内容）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VersionInfo {
    /// 版本号。
    pub version: u64,
    /// 父版本。
    pub parent_version: Option<u64>,
    /// 作者用户 ID。
    pub author_id: Option<i64>,
    /// 创建时间。
    pub created_at: Timestamp,
    /// 说明。
    pub comment: String,
    /// 内容哈希。
    pub content_sha256: String,
}

/// 配置对象概要。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ObjectInfo {
    /// 对象 ID。
    pub id: i64,
    /// 种类（蛇形名称）。
    pub kind: String,
    /// 名称（同种类内唯一）。
    pub name: String,
    /// 头版本。
    pub head_version: u64,
}

/// 配置版本链句柄。
#[derive(Debug, Clone, Copy)]
pub struct Configs<'store> {
    /// 所属存储。
    store: &'store Store,
}

impl<'store> Configs<'store> {
    /// 构造句柄。
    pub(crate) const fn new(store: &'store Store) -> Self {
        Self { store }
    }

    /// 新建对象并写入版本 1。
    ///
    /// # Errors
    ///
    /// 名称重复、校验失败或写入失败时返回错误。
    pub fn create<T: ConfigDocument>(
        self,
        name: &str,
        doc: &T,
        meta: &Authorship,
    ) -> StoreResult<VersionRef> {
        if name.is_empty() || name.len() > 128 {
            return Err(StoreError::Invalid("名称长度必须为 1–128".to_owned()));
        }
        self.store.write(|tx| {
            exec(
                tx,
                "INSERT INTO config_objects (kind, name, head_version) VALUES (?1, ?2, 0)",
                [T::KIND.as_str(), name],
            )?;
            let object_id = tx.last_insert_rowid();
            commit_doc(tx, object_id, doc, 0, meta)
        })
    }

    /// 保存新内容。调用方必须携带读到的头版本 `expected_head`。
    ///
    /// 内容与头版本相同则直接返回头版本，不产生新版本。
    ///
    /// # Errors
    ///
    /// 头版本已变化时返回 [`StoreError::Conflict`]；校验失败或写入失败时返回错误。
    pub fn save<T: ConfigDocument>(
        self,
        object_id: i64,
        doc: &T,
        expected_head: u64,
        meta: &Authorship,
    ) -> StoreResult<VersionRef> {
        self.store
            .write(|tx| commit_doc(tx, object_id, doc, expected_head, meta))
    }

    /// 把 `to_version` 的内容作为新版本保存，说明自动标注来源版本号。
    ///
    /// # Errors
    ///
    /// 版本不存在、头版本冲突或写入失败时返回错误。
    pub fn rollback<T: ConfigDocument>(
        self,
        object_id: i64,
        to_version: u64,
        expected_head: u64,
        meta: &Authorship,
    ) -> StoreResult<VersionRef> {
        self.store.write(|tx| {
            let doc: T = load_version(tx, object_id, to_version)?;
            let annotated = Authorship {
                author_id: meta.author_id,
                comment: format!("回滚到版本 {to_version}：{}", meta.comment),
                at: meta.at,
            };
            commit_doc(tx, object_id, &doc, expected_head, &annotated)
        })
    }

    /// 读取头版本。
    ///
    /// # Errors
    ///
    /// 对象不存在、种类不符或内容无法解码时返回错误。
    pub fn head<T: ConfigDocument>(self, object_id: i64) -> StoreResult<(VersionRef, T)> {
        self.store.read(|conn| {
            let (kind, head) = object_head(conn, object_id)?;
            check_kind::<T>(&kind)?;
            let reference = version_ref(conn, object_id, head)?;
            let doc = load_version(conn, object_id, head)?;
            Ok((reference, doc))
        })
    }

    /// 读取指定版本。
    ///
    /// # Errors
    ///
    /// 版本不存在或内容无法解码时返回错误。
    pub fn load<T: ConfigDocument>(self, object_id: i64, version: u64) -> StoreResult<T> {
        self.store.read(|conn| {
            let (kind, _head) = object_head(conn, object_id)?;
            check_kind::<T>(&kind)?;
            load_version(conn, object_id, version)
        })
    }

    /// 两个版本之间的结构化差异。
    ///
    /// # Errors
    ///
    /// 版本不存在或内容无法解码时返回错误。
    pub fn diff<T: ConfigDocument>(
        self,
        object_id: i64,
        from: u64,
        to: u64,
    ) -> StoreResult<Vec<Change>> {
        let left: T = self.load(object_id, from)?;
        let right: T = self.load(object_id, to)?;
        Ok(diff(&canonical_json(&left)?, &canonical_json(&right)?))
    }

    /// 版本历史（从新到旧）。
    ///
    /// # Errors
    ///
    /// 查询失败时返回错误。
    pub fn versions(self, object_id: i64) -> StoreResult<Vec<VersionInfo>> {
        self.store.read(|conn| {
            query_vec(
                conn,
                "SELECT version, parent_version, author_id, created_at, comment, content_sha256
                 FROM config_versions WHERE object_id = ?1 ORDER BY version DESC",
                [object_id],
                |row| {
                    Ok(VersionInfo {
                        version: to_u64(row.get(0)?)?,
                        parent_version: row.get::<_, Option<i64>>(1)?.map(to_u64).transpose()?,
                        author_id: row.get(2)?,
                        created_at: parse_ts(&row.get::<_, String>(3)?)?,
                        comment: row.get(4)?,
                        content_sha256: row.get(5)?,
                    })
                },
            )
        })
    }

    /// 列出某种类的全部对象。
    ///
    /// # Errors
    ///
    /// 查询失败时返回错误。
    pub fn list(self, kind: xpxe_domain::ConfigKind) -> StoreResult<Vec<ObjectInfo>> {
        self.store.read(|conn| {
            query_vec(
                conn,
                "SELECT id, kind, name, head_version FROM config_objects WHERE kind = ?1 ORDER BY name",
                [kind.as_str()],
                |row| {
                    Ok(ObjectInfo {
                        id: row.get(0)?,
                        kind: row.get(1)?,
                        name: row.get(2)?,
                        head_version: to_u64(row.get(3)?)?,
                    })
                },
            )
        })
    }
}

/// 在事务内写入新版本（或在内容未变时返回头版本）。
///
/// # Errors
///
/// 冲突、校验失败或写入失败时返回错误。
fn commit_doc<T: ConfigDocument>(
    tx: &Transaction<'_>,
    object_id: i64,
    doc: &T,
    expected_head: u64,
    meta: &Authorship,
) -> StoreResult<VersionRef> {
    let (kind, head) = object_head(tx, object_id)?;
    check_kind::<T>(&kind)?;
    if head != expected_head {
        return Err(StoreError::Conflict { current_head: head });
    }
    let canonical = canonical_json(doc)?;
    let sha = content_sha256_hex(&canonical);
    if head > 0 {
        let current = version_ref(tx, object_id, head)?;
        if current.content_sha256 == sha {
            return Ok(current);
        }
    }
    let version = head
        .checked_add(1)
        .ok_or_else(|| StoreError::Corrupt("版本号溢出".to_owned()))?;
    let parent = if head == 0 { None } else { Some(to_i64(head)?) };
    exec(
        tx,
        "INSERT INTO config_versions (object_id, version, parent_version, author_id, created_at,
           comment, schema, content, content_sha256)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        rusqlite::params![
            object_id,
            to_i64(version)?,
            parent,
            meta.author_id,
            ts(meta.at)?,
            meta.comment,
            T::SCHEMA,
            canonical.as_str()?,
            sha,
        ],
    )?;
    exec(
        tx,
        "UPDATE config_objects SET head_version = ?2 WHERE id = ?1",
        rusqlite::params![object_id, to_i64(version)?],
    )?;
    Ok(VersionRef {
        object_id,
        version,
        content_sha256: sha,
    })
}

/// 对象种类与头版本。
///
/// # Errors
///
/// 对象不存在时返回错误。
fn object_head(conn: &Connection, object_id: i64) -> StoreResult<(String, u64)> {
    query_opt(
        conn,
        "SELECT kind, head_version FROM config_objects WHERE id = ?1",
        [object_id],
        |row| Ok((row.get::<_, String>(0)?, to_u64(row.get(1)?)?)),
    )?
    .ok_or(StoreError::NotFound("配置对象"))
}

/// 种类必须与文档类型一致。
///
/// # Errors
///
/// 不一致时返回错误。
fn check_kind<T: ConfigDocument>(kind: &str) -> StoreResult<()> {
    if kind == T::KIND.as_str() {
        Ok(())
    } else {
        Err(StoreError::Invalid(format!(
            "对象种类为 {kind}，不是 {}",
            T::KIND
        )))
    }
}

/// 某版本的引用。
///
/// # Errors
///
/// 版本不存在时返回错误。
fn version_ref(conn: &Connection, object_id: i64, version: u64) -> StoreResult<VersionRef> {
    let sha = query_opt(
        conn,
        "SELECT content_sha256 FROM config_versions WHERE object_id = ?1 AND version = ?2",
        rusqlite::params![object_id, to_i64(version)?],
        |row| Ok(row.get::<_, String>(0)?),
    )?;
    Ok(VersionRef {
        object_id,
        version,
        content_sha256: sha.ok_or(StoreError::NotFound("配置版本"))?,
    })
}

/// 读取并（必要时）迁移某版本内容。
///
/// # Errors
///
/// 版本不存在或内容无法解码时返回错误。
fn load_version<T: ConfigDocument>(
    conn: &Connection,
    object_id: i64,
    version: u64,
) -> StoreResult<T> {
    let (schema, content) = query_opt(
        conn,
        "SELECT schema, content FROM config_versions WHERE object_id = ?1 AND version = ?2",
        rusqlite::params![object_id, to_i64(version)?],
        |row| Ok((row.get::<_, u32>(0)?, row.get::<_, String>(1)?)),
    )?
    .ok_or(StoreError::NotFound("配置版本"))?;
    let value: Value = serde_json::from_str(&content)?;
    Ok(T::migrate(schema, value)?)
}
