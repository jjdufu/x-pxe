//! 迁移单元测试。

use rusqlite::Connection;
use xpxe_test_support::{TestResult, ensure, ensure_eq};

use super::{MIGRATIONS, current_version, run, run_until};
use crate::integrity_messages;

/// 每个迁移版本之后可插入的样例数据（下标 = 已执行迁移数 - 1）。
const SAMPLES: &[&str] = &[include_str!("samples/after_0001.sql")];

/// 空库迁移到最新。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn empty_to_latest() -> TestResult {
    let mut conn = Connection::open_in_memory()?;
    run(&mut conn)?;
    ensure_eq(&current_version(&conn)?, &MIGRATIONS.len(), "最新版本")?;
    run(&mut conn)?;
    ensure_eq(
        &current_version(&conn)?,
        &MIGRATIONS.len(),
        "重复执行无副作用",
    )?;
    ensure(integrity_messages(&conn)?.is_empty(), "完整性")?;
    Ok(())
}

/// 每个迁移都能在含样例数据的库上执行。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn each_migration_runs_on_sample_data() -> TestResult {
    ensure_eq(&SAMPLES.len(), &MIGRATIONS.len(), "每个迁移都有样例")?;
    for (index, sample) in SAMPLES.iter().enumerate() {
        let mut conn = Connection::open_in_memory()?;
        let done = index.checked_add(1).ok_or("溢出")?;
        run_until(&mut conn, done)?;
        conn.execute_batch(sample)?;
        run(&mut conn)?;
        ensure_eq(&current_version(&conn)?, &MIGRATIONS.len(), "迁移到最新")?;
        ensure(integrity_messages(&conn)?.is_empty(), "完整性")?;
        let users = crate::sqlval::query_i64(&conn, "SELECT count(*) FROM users", [])?;
        ensure(users >= 1, "样例数据保留")?;
    }
    Ok(())
}

/// 库版本比程序新时拒绝。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn newer_schema_is_rejected() -> TestResult {
    let mut conn = Connection::open_in_memory()?;
    conn.pragma_update(None, "user_version", 999_i64)?;
    ensure(run(&mut conn).is_err(), "拒绝更新的 schema")?;
    Ok(())
}
