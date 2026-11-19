//! 迁移：`migrations/NNNN_*.sql` 编入二进制，按 `PRAGMA user_version` 顺序执行，每个迁移一个事务。

use rusqlite::{Connection, TransactionBehavior};

use crate::{StoreError, StoreResult};

/// 全部迁移，下标 + 1 即迁移后的 `user_version`。
pub(crate) const MIGRATIONS: &[(&str, &str)] = &[
    ("0001_init", include_str!("../migrations/0001_init.sql")),
    ("0002_outbox", include_str!("../migrations/0002_outbox.sql")),
    ("0003_iso", include_str!("../migrations/0003_iso.sql")),
];

/// 把数据库迁移到最新版本。
///
/// # Errors
///
/// 库版本比程序新或任一迁移失败时返回错误（失败的迁移整体回滚）。
pub(crate) fn run(conn: &mut Connection) -> StoreResult<()> {
    run_until(conn, MIGRATIONS.len())
}

/// 把数据库迁移到第 `target` 个迁移（含）。
///
/// # Errors
///
/// 库版本比 `target` 新或任一迁移失败时返回错误。
pub(crate) fn run_until(conn: &mut Connection, target: usize) -> StoreResult<()> {
    let current = current_version(conn)?;
    if current > MIGRATIONS.len() {
        return Err(StoreError::Corrupt(format!(
            "数据库 schema {current} 比程序支持的 {} 新",
            MIGRATIONS.len()
        )));
    }
    for (index, (name, sql)) in MIGRATIONS.iter().enumerate().take(target).skip(current) {
        let version = index
            .checked_add(1)
            .ok_or_else(|| StoreError::Corrupt("迁移序号溢出".to_owned()))?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Exclusive)?;
        tx.execute_batch(sql)
            .map_err(|error| StoreError::Sqlite(format!("迁移 {name}：{error}")))?;
        tx.pragma_update(
            None,
            "user_version",
            i64::try_from(version).unwrap_or(i64::MAX),
        )?;
        tx.commit()?;
    }
    Ok(())
}

/// 当前 `user_version`。
///
/// # Errors
///
/// 查询失败或为负时返回错误。
pub(crate) fn current_version(conn: &Connection) -> StoreResult<usize> {
    let raw = crate::sqlval::query_i64(conn, "PRAGMA user_version", [])?;
    usize::try_from(raw).map_err(|error| StoreError::Corrupt(error.to_string()))
}

#[cfg(test)]
mod tests;
