//! 列值转换与密码学小工具：RFC 3339 时间、整数换算、随机数、哈希、常量时间比较。

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ring::rand::{SecureRandom as _, SystemRandom};
use sha2::{Digest as _, Sha256};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use xpxe_domain::Timestamp;

use crate::{StoreError, StoreResult};

/// 把时间戳写成定长 RFC 3339（UTC，`Z` 结尾，秒精度）。
///
/// # Errors
///
/// 时间超出可表示范围时返回错误。
pub(crate) fn ts(value: Timestamp) -> StoreResult<String> {
    let secs = i64::try_from(value.unix_seconds())
        .map_err(|error| StoreError::Invalid(error.to_string()))?;
    let at = OffsetDateTime::from_unix_timestamp(secs)
        .map_err(|error| StoreError::Invalid(error.to_string()))?;
    at.format(&Rfc3339)
        .map_err(|error| StoreError::Invalid(error.to_string()))
}

/// 解析 RFC 3339 时间。
///
/// # Errors
///
/// 文本不是合法 RFC 3339 或早于 1970 年时返回错误。
pub(crate) fn parse_ts(raw: &str) -> StoreResult<Timestamp> {
    let at = OffsetDateTime::parse(raw, &Rfc3339)
        .map_err(|error| StoreError::Corrupt(format!("时间 {raw}：{error}")))?;
    let secs = u64::try_from(at.unix_timestamp())
        .map_err(|error| StoreError::Corrupt(format!("时间 {raw}：{error}")))?;
    Ok(Timestamp::from_unix_seconds(secs))
}

/// 可选时间列。
///
/// # Errors
///
/// 文本不合法时返回错误。
pub(crate) fn parse_opt_ts(raw: Option<&str>) -> StoreResult<Option<Timestamp>> {
    raw.map(parse_ts).transpose()
}

/// `u64` 转 `SQLite` 整数。
///
/// # Errors
///
/// 超出 `i64` 时返回错误。
pub(crate) fn to_i64(value: u64) -> StoreResult<i64> {
    i64::try_from(value).map_err(|error| StoreError::Invalid(error.to_string()))
}

/// `SQLite` 整数转 `u64`。
///
/// # Errors
///
/// 为负时返回错误。
pub(crate) fn to_u64(value: i64) -> StoreResult<u64> {
    u64::try_from(value).map_err(|error| StoreError::Corrupt(error.to_string()))
}

/// 执行一条不关心影响行数的语句。
///
/// # Errors
///
/// 语句失败时返回错误。
pub(crate) fn exec<P: rusqlite::Params>(
    conn: &rusqlite::Connection,
    sql: &str,
    params: P,
) -> StoreResult<()> {
    let _changed = conn.execute(sql, params)?;
    Ok(())
}

/// 查询至多一行并映射。
///
/// # Errors
///
/// 查询或映射失败时返回错误。
pub(crate) fn query_opt<T, P, F>(
    conn: &rusqlite::Connection,
    sql: &str,
    params: P,
    map: F,
) -> StoreResult<Option<T>>
where
    P: rusqlite::Params,
    F: FnOnce(&rusqlite::Row<'_>) -> StoreResult<T>,
{
    let mut stmt = conn.prepare(sql)?;
    let mut rows = stmt.query(params)?;
    rows.next()?.map(map).transpose()
}

/// 查询多行并逐行映射。
///
/// # Errors
///
/// 查询或映射失败时返回错误。
pub(crate) fn query_vec<T, P, F>(
    conn: &rusqlite::Connection,
    sql: &str,
    params: P,
    mut map: F,
) -> StoreResult<Vec<T>>
where
    P: rusqlite::Params,
    F: FnMut(&rusqlite::Row<'_>) -> StoreResult<T>,
{
    let mut stmt = conn.prepare(sql)?;
    let mut rows = stmt.query(params)?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        out.push(map(row)?);
    }
    Ok(out)
}

/// 查询一个整数（`count(*)`、PRAGMA 等）。
///
/// # Errors
///
/// 查询失败或没有结果时返回错误。
pub(crate) fn query_i64<P: rusqlite::Params>(
    conn: &rusqlite::Connection,
    sql: &str,
    params: P,
) -> StoreResult<i64> {
    query_opt(conn, sql, params, |row| Ok(row.get::<_, i64>(0)?))?
        .ok_or_else(|| StoreError::Corrupt(format!("查询无结果：{sql}")))
}

/// 用系统 CSPRNG 生成 `N` 字节。
///
/// # Errors
///
/// 随机源不可用时返回错误。
pub(crate) fn random<const N: usize>() -> StoreResult<[u8; N]> {
    let mut out = [0_u8; N];
    SystemRandom::new()
        .fill(&mut out)
        .map_err(|_unspecified| StoreError::Crypto)?;
    Ok(out)
}

/// 32 字节随机数的 base64url（无填充）文本。
///
/// # Errors
///
/// 随机源不可用时返回错误。
pub(crate) fn random_token() -> StoreResult<String> {
    Ok(URL_SAFE_NO_PAD.encode(random::<32>()?))
}

/// SHA-256 十六进制。
#[must_use]
pub(crate) fn sha256_hex(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}

/// 常量时间比较两个字节串（长度不同直接返回假，长度不是秘密）。
#[must_use]
pub(crate) fn ct_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let diff = left
        .iter()
        .zip(right)
        .fold(0_u8, |acc, (lhs, rhs)| acc | (lhs ^ rhs));
    diff == 0
}

#[cfg(test)]
mod tests;
