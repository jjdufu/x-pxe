//! 审计哈希链：`hash = SHA-256(prev_hash ‖ canonical_json(entry))`，首条 `prev_hash` 为 32 字节零。

use std::io::Write;

use rusqlite::{Connection, Row, Transaction};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use xpxe_domain::Timestamp;

use crate::sqlval::{exec, query_opt, query_vec, ts};
use crate::{Store, StoreError, StoreResult};

/// 首条记录的 `prev_hash`（32 字节零的十六进制）。
const GENESIS: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// 一条待追加的审计记录。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditRecord {
    /// 操作者（用户名、`system`、令牌名 …）。
    pub actor: String,
    /// 来源 IP。
    pub ip: String,
    /// 动作。
    pub action: String,
    /// 目标。
    pub target: String,
    /// 结果（`ok` / `denied` / `error` …）。
    pub outcome: String,
    /// 详情。
    pub detail: Value,
}

/// 链校验结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChainReport {
    /// 已校验的记录数。
    pub checked: u64,
    /// 第一处断链的记录 ID。
    pub first_broken: Option<i64>,
}

/// 审计句柄。
#[derive(Debug, Clone, Copy)]
pub struct Audit<'store> {
    /// 所属存储。
    store: &'store Store,
}

/// 一行审计记录（含哈希）。
#[derive(Debug)]
struct StoredEntry {
    /// ID。
    id: i64,
    /// 时间文本。
    at: String,
    /// 记录内容。
    record: AuditRecord,
    /// 前一条哈希。
    prev_hash: String,
    /// 本条哈希。
    hash: String,
}

impl<'store> Audit<'store> {
    /// 构造句柄。
    pub(crate) const fn new(store: &'store Store) -> Self {
        Self { store }
    }

    /// 追加一条记录，返回其 ID。
    ///
    /// # Errors
    ///
    /// 写入失败时返回错误。
    pub fn append(self, record: &AuditRecord, now: Timestamp) -> StoreResult<i64> {
        self.store.write(|tx| append_tx(tx, record, now))
    }

    /// 从头重算整条链，返回第一处断链位置。
    ///
    /// # Errors
    ///
    /// 查询失败时返回错误。
    pub fn verify(self) -> StoreResult<ChainReport> {
        self.store.read(|conn| {
            let entries = query_vec(conn, &format!("{SELECT_ENTRY} ORDER BY id"), [], raw_entry)?;
            walk_chain(entries)
        })
    }

    /// 把 `since` 之后（含）的记录以 JSON Lines 写到 `out`，返回条数。
    ///
    /// # Errors
    ///
    /// 查询或写出失败时返回错误。
    pub fn export_since<W: Write>(self, since: Timestamp, out: &mut W) -> StoreResult<u64> {
        let since_text = ts(since)?;
        self.store.read(|conn| export(conn, &since_text, out))
    }
}

/// 在已有事务内追加一条审计记录，返回其 ID。
///
/// # Errors
///
/// 写入失败时返回错误。
pub(crate) fn append_tx(
    tx: &Transaction<'_>,
    record: &AuditRecord,
    now: Timestamp,
) -> StoreResult<i64> {
    let at = ts(now)?;
    let prev = query_opt(
        tx,
        "SELECT id, hash FROM audit_log ORDER BY id DESC LIMIT 1",
        [],
        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
    )?;
    let (id, prev_hash) = match prev {
        Some((last, hash)) => (
            last.checked_add(1)
                .ok_or_else(|| StoreError::Corrupt("审计 ID 溢出".to_owned()))?,
            hash,
        ),
        None => (1, GENESIS.to_owned()),
    };
    let hash = chain_hash(&prev_hash, id, &at, record)?;
    exec(tx,
        "INSERT INTO audit_log (id, at, actor, ip, action, target, outcome, detail, prev_hash, hash)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        rusqlite::params![
            id,
            at,
            record.actor,
            record.ip,
            record.action,
            record.target,
            record.outcome,
            serde_json::to_string(&record.detail)?,
            prev_hash,
            hash,
        ],
    )?;
    Ok(id)
}

/// 依次核对每条记录的 `prev_hash` 与重算哈希。
///
/// # Errors
///
/// 哈希无法计算时返回错误。
fn walk_chain(entries: Vec<StoredEntry>) -> StoreResult<ChainReport> {
    let mut expected_prev = GENESIS.to_owned();
    let mut checked = 0_u64;
    for entry in entries {
        let intact = entry.prev_hash == expected_prev
            && chain_hash(&entry.prev_hash, entry.id, &entry.at, &entry.record)? == entry.hash;
        if !intact {
            return Ok(ChainReport {
                checked,
                first_broken: Some(entry.id),
            });
        }
        expected_prev = entry.hash;
        checked = checked.saturating_add(1);
    }
    Ok(ChainReport {
        checked,
        first_broken: None,
    })
}

/// 审计查询的公共列。
const SELECT_ENTRY: &str =
    "SELECT id, at, actor, ip, action, target, outcome, detail, prev_hash, hash FROM audit_log";

/// 导出实现。
///
/// # Errors
///
/// 查询或写出失败时返回错误。
fn export<W: Write>(conn: &Connection, since: &str, out: &mut W) -> StoreResult<u64> {
    let entries = query_vec(
        conn,
        &format!("{SELECT_ENTRY} WHERE at >= ?1 ORDER BY id"),
        [since],
        raw_entry,
    )?;
    let mut count = 0_u64;
    for entry in entries {
        let line = json!({
            "id": entry.id,
            "at": entry.at,
            "actor": entry.record.actor,
            "ip": entry.record.ip,
            "action": entry.record.action,
            "target": entry.record.target,
            "outcome": entry.record.outcome,
            "detail": entry.record.detail,
            "prev_hash": entry.prev_hash,
            "hash": entry.hash,
        });
        writeln!(out, "{line}")?;
        count = count.saturating_add(1);
    }
    Ok(count)
}

/// 行 → 审计记录。`detail` 无法解析时保留为字符串，使篡改能被 `verify` 发现而不是报错。
///
/// # Errors
///
/// 列类型不符时返回错误。
fn raw_entry(row: &Row<'_>) -> StoreResult<StoredEntry> {
    let detail: String = row.get(7)?;
    Ok(StoredEntry {
        id: row.get(0)?,
        at: row.get(1)?,
        record: AuditRecord {
            actor: row.get(2)?,
            ip: row.get(3)?,
            action: row.get(4)?,
            target: row.get(5)?,
            outcome: row.get(6)?,
            detail: serde_json::from_str(&detail).unwrap_or(Value::String(detail)),
        },
        prev_hash: row.get(8)?,
        hash: row.get(9)?,
    })
}

/// 计算链哈希：对象键排序后的紧凑 JSON 作为规范化形式。
///
/// # Errors
///
/// 序列化失败或 `prev_hash` 不是十六进制时返回错误。
fn chain_hash(prev_hash: &str, id: i64, at: &str, record: &AuditRecord) -> StoreResult<String> {
    let entry = json!({
        "id": id,
        "at": at,
        "actor": record.actor,
        "ip": record.ip,
        "action": record.action,
        "target": record.target,
        "outcome": record.outcome,
        "detail": record.detail,
    });
    let prev = hex::decode(prev_hash).map_err(|error| StoreError::Corrupt(error.to_string()))?;
    let mut hasher = Sha256::new();
    hasher.update(&prev);
    hasher.update(serde_json::to_vec(&entry)?);
    Ok(hex::encode(hasher.finalize()))
}
