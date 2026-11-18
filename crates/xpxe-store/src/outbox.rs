//! 发件箱：与状态转换同一事务写入的副作用，由控制面 worker 执行、失败按退避重试。

use rusqlite::{Row, Transaction};
use serde::Serialize;
use serde_json::Value;
use xpxe_domain::{SessionId, Timestamp};

use crate::sqlval::{exec, parse_ts, query_opt, query_vec, ts};
use crate::{Store, StoreError, StoreResult};

/// 待写入的一条副作用。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboxItem {
    /// 节点。
    pub node_id: i64,
    /// 关联会话。
    pub session_id: Option<SessionId>,
    /// 副作用名（如 `publish_directives`、`eject_media`、`close_session`）。
    pub effect: String,
    /// 附加数据。
    pub payload: Value,
}

/// 已入库的一条副作用。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OutboxEntry {
    /// 行 ID。
    pub id: i64,
    /// 节点。
    pub node_id: i64,
    /// 关联会话。
    pub session_id: Option<String>,
    /// 副作用名。
    pub effect: String,
    /// 附加数据。
    pub payload: Value,
    /// 已尝试次数。
    pub attempts: u32,
    /// 下次执行时间。
    pub next_at: Timestamp,
}

/// 发件箱句柄。
#[derive(Debug, Clone, Copy)]
pub struct Outbox<'store> {
    /// 存储。
    store: &'store Store,
}

impl<'store> Outbox<'store> {
    /// 构造。
    pub(crate) const fn new(store: &'store Store) -> Self {
        Self { store }
    }

    /// 到期未完成的条目（按 ID 升序）。
    ///
    /// # Errors
    ///
    /// 查询失败时返回错误。
    pub fn due(self, now: Timestamp, limit: u32) -> StoreResult<Vec<OutboxEntry>> {
        let at = ts(now)?;
        self.store.read(|conn| {
            query_vec(
                conn,
                "SELECT id, node_id, session_id, effect, payload, attempts, next_at FROM outbox
                 WHERE done_at IS NULL AND next_at <= ?1 ORDER BY id LIMIT ?2",
                rusqlite::params![at, limit],
                entry_row,
            )
        })
    }

    /// 最早的未完成条目的执行时间。
    ///
    /// # Errors
    ///
    /// 查询失败时返回错误。
    pub fn next_due(self) -> StoreResult<Option<Timestamp>> {
        self.store.read(|conn| {
            let raw = query_opt(
                conn,
                "SELECT min(next_at) FROM outbox WHERE done_at IS NULL",
                [],
                |row| Ok(row.get::<_, Option<String>>(0)?),
            )?;
            raw.flatten().as_deref().map(parse_ts).transpose()
        })
    }

    /// 标记完成。
    ///
    /// # Errors
    ///
    /// 写入失败时返回错误。
    pub fn complete(self, id: i64, now: Timestamp) -> StoreResult<()> {
        let at = ts(now)?;
        self.store.write(|tx| {
            exec(
                tx,
                "UPDATE outbox SET done_at = ?2, attempts = attempts + 1, last_error = NULL WHERE id = ?1",
                rusqlite::params![id, at],
            )?;
            Ok(())
        })
    }

    /// 记录失败并安排下次重试。
    ///
    /// # Errors
    ///
    /// 写入失败时返回错误。
    pub fn defer(self, id: i64, error: &str, next_at: Timestamp) -> StoreResult<()> {
        let at = ts(next_at)?;
        self.store.write(|tx| {
            exec(
                tx,
                "UPDATE outbox SET attempts = attempts + 1, last_error = ?2, next_at = ?3 WHERE id = ?1",
                rusqlite::params![id, error, at],
            )?;
            Ok(())
        })
    }
}

/// 在已有事务内写入一条副作用。
///
/// # Errors
///
/// 写入失败时返回错误。
pub(crate) fn insert_tx(
    tx: &Transaction<'_>,
    item: &OutboxItem,
    now: Timestamp,
) -> StoreResult<()> {
    let at = ts(now)?;
    exec(
        tx,
        "INSERT INTO outbox (node_id, session_id, effect, payload, next_at, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
        rusqlite::params![
            item.node_id,
            item.session_id.as_ref().map(SessionId::as_str),
            item.effect,
            serde_json::to_string(&item.payload)?,
            at,
        ],
    )?;
    Ok(())
}

/// 行 → 条目。
///
/// # Errors
///
/// 列内容不合法时返回错误。
fn entry_row(row: &Row<'_>) -> StoreResult<OutboxEntry> {
    let payload: String = row.get(4)?;
    let attempts: i64 = row.get(5)?;
    Ok(OutboxEntry {
        id: row.get(0)?,
        node_id: row.get(1)?,
        session_id: row.get(2)?,
        effect: row.get(3)?,
        payload: serde_json::from_str(&payload)?,
        attempts: u32::try_from(attempts)
            .map_err(|error| StoreError::Corrupt(error.to_string()))?,
        next_at: parse_ts(&row.get::<_, String>(6)?)?,
    })
}
