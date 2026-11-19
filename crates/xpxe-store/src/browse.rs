//! 管理界面的只读浏览查询：用户、会话、API Token、审计分页、跨节点事件与装机结果统计。

use rusqlite::{Connection, Row};
use serde::Serialize;
use serde_json::Value;
use xpxe_domain::{NodeState, Timestamp};

use crate::auth::Role;
use crate::sqlval::{exec, parse_opt_ts, parse_ts, query_i64, query_vec, to_u64, ts};
use crate::{StoreError, StoreResult};

/// 会话标识前缀长度（取会话 ID 哈希的前 16 位十六进制，不暴露 Cookie 原值）。
pub const SESSION_KEY_LEN: usize = 16;
/// 审计分页上限。
pub const AUDIT_PAGE_MAX: u32 = 500;

/// 用户列表项。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UserSummary {
    /// 用户 ID。
    pub id: i64,
    /// 用户名。
    pub username: String,
    /// 角色。
    pub role: Role,
    /// 下次登录必须改密。
    pub must_change_password: bool,
    /// 已禁用。
    pub disabled: bool,
    /// 创建时间。
    pub created_at: Timestamp,
}

/// 用户修改（缺省字段不变）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct UserPatch {
    /// 新角色。
    pub role: Option<Role>,
    /// 是否禁用。
    pub disabled: Option<bool>,
}

/// 活动会话。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SessionInfo {
    /// 会话标识（ID 哈希前缀）。
    pub key: String,
    /// 创建时间。
    pub created_at: Timestamp,
    /// 最近活动。
    pub last_seen_at: Timestamp,
    /// 绝对过期时间。
    pub expires_at: Timestamp,
    /// 来源 IP。
    pub ip: String,
    /// User-Agent。
    pub user_agent: String,
}

/// API Token 列表项（不含令牌原值与哈希）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TokenInfo {
    /// Token ID。
    pub id: i64,
    /// 名称。
    pub name: String,
    /// 作用域。
    pub scopes: Vec<String>,
    /// 创建时间。
    pub created_at: Timestamp,
    /// 过期时间。
    pub expires_at: Timestamp,
    /// 最近使用。
    pub last_used_at: Option<Timestamp>,
}

/// 审计筛选（`None` 表示不限）。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AuditQuery {
    /// 操作者。
    pub actor: Option<String>,
    /// 动作。
    pub action: Option<String>,
    /// 结果。
    pub outcome: Option<String>,
    /// 起始时间（含）。
    pub since: Option<Timestamp>,
    /// 截止时间（含）。
    pub until: Option<Timestamp>,
    /// 偏移。
    pub offset: u32,
    /// 条数（上限 [`AUDIT_PAGE_MAX`]）。
    pub limit: u32,
}

/// 一条审计记录。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AuditEntry {
    /// ID。
    pub id: i64,
    /// 时间。
    pub at: String,
    /// 操作者。
    pub actor: String,
    /// 来源 IP。
    pub ip: String,
    /// 动作。
    pub action: String,
    /// 对象。
    pub target: String,
    /// 结果。
    pub outcome: String,
    /// 详情。
    pub detail: Value,
}

/// 审计分页结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AuditPage {
    /// 满足筛选的总条数。
    pub total: u64,
    /// 本页记录（从新到旧）。
    pub entries: Vec<AuditEntry>,
    /// 出现过的操作者（筛选候选）。
    pub actors: Vec<String>,
    /// 出现过的动作（筛选候选）。
    pub actions: Vec<String>,
}

/// 带详情的节点事件。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EventDetail {
    /// 行 ID。
    pub id: i64,
    /// 节点 ID。
    pub node_id: i64,
    /// 节点主机名。
    pub hostname: String,
    /// 会话。
    pub session_id: Option<String>,
    /// 事件 ID。
    pub event_id: String,
    /// 时间。
    pub at: Timestamp,
    /// 来源。
    pub source: String,
    /// 种类。
    pub kind: String,
    /// 转换前。
    pub from_state: NodeState,
    /// 转换后。
    pub to_state: NodeState,
    /// 附加数据。
    pub payload: Value,
}

/// 某天的装机结果计数。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DayOutcome {
    /// 日期（UTC，`YYYY-MM-DD`）。
    pub day: String,
    /// 到达 `Ready` 的次数。
    pub ready: u64,
    /// 到达 `Failed` 的次数。
    pub failed: u64,
}

/// 读取用户列表。
///
/// # Errors
///
/// 查询失败时返回错误。
pub(crate) fn users(conn: &Connection) -> StoreResult<Vec<UserSummary>> {
    query_vec(
        conn,
        "SELECT id, username, role, must_change_password, disabled, created_at FROM users ORDER BY id",
        [],
        |row| {
            Ok(UserSummary {
                id: row.get(0)?,
                username: row.get(1)?,
                role: row.get::<_, String>(2)?.parse()?,
                must_change_password: row.get(3)?,
                disabled: row.get(4)?,
                created_at: parse_ts(&row.get::<_, String>(5)?)?,
            })
        },
    )
}

/// 修改用户：禁用时同时删除其会话。返回是否命中。
///
/// # Errors
///
/// 写入失败时返回错误。
pub(crate) fn patch_user(
    tx: &rusqlite::Transaction<'_>,
    user_id: i64,
    patch: UserPatch,
) -> StoreResult<bool> {
    let changed = tx.execute(
        "UPDATE users SET role = coalesce(?2, role), disabled = coalesce(?3, disabled) WHERE id = ?1",
        rusqlite::params![user_id, patch.role.map(Role::as_str), patch.disabled],
    )?;
    if patch.disabled == Some(true) {
        exec(tx, "DELETE FROM sessions WHERE user_id = ?1", [user_id])?;
    }
    Ok(changed > 0)
}

/// 某用户的会话列表（最近活动在前）。
///
/// # Errors
///
/// 查询失败时返回错误。
pub(crate) fn sessions(conn: &Connection, user_id: i64) -> StoreResult<Vec<SessionInfo>> {
    query_vec(
        conn,
        "SELECT substr(id_hash, 1, 16), created_at, last_seen_at, expires_at, ip, user_agent
         FROM sessions WHERE user_id = ?1 ORDER BY last_seen_at DESC",
        [user_id],
        |row| {
            Ok(SessionInfo {
                key: row.get(0)?,
                created_at: parse_ts(&row.get::<_, String>(1)?)?,
                last_seen_at: parse_ts(&row.get::<_, String>(2)?)?,
                expires_at: parse_ts(&row.get::<_, String>(3)?)?,
                ip: row.get(4)?,
                user_agent: row.get(5)?,
            })
        },
    )
}

/// 按标识删除某用户的会话。
///
/// # Errors
///
/// 标识长度不对或写入失败时返回错误。
pub(crate) fn revoke_session(
    tx: &rusqlite::Transaction<'_>,
    user_id: i64,
    key: &str,
) -> StoreResult<bool> {
    if key.len() != SESSION_KEY_LEN || !key.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(StoreError::Invalid("会话标识不合法".to_owned()));
    }
    Ok(tx.execute(
        "DELETE FROM sessions WHERE user_id = ?1 AND substr(id_hash, 1, 16) = ?2",
        rusqlite::params![user_id, key],
    )? > 0)
}

/// 某用户的 API Token 列表。
///
/// # Errors
///
/// 查询失败时返回错误。
pub(crate) fn tokens(conn: &Connection, user_id: i64) -> StoreResult<Vec<TokenInfo>> {
    query_vec(
        conn,
        "SELECT id, name, scopes, created_at, expires_at, last_used_at
         FROM api_tokens WHERE user_id = ?1 ORDER BY id DESC",
        [user_id],
        |row| {
            Ok(TokenInfo {
                id: row.get(0)?,
                name: row.get(1)?,
                scopes: row
                    .get::<_, String>(2)?
                    .split(',')
                    .map(str::to_owned)
                    .collect(),
                created_at: parse_ts(&row.get::<_, String>(3)?)?,
                expires_at: parse_ts(&row.get::<_, String>(4)?)?,
                last_used_at: parse_opt_ts(row.get::<_, Option<String>>(5)?.as_deref())?,
            })
        },
    )
}

/// 审计筛选条件（`?1`–`?5`）。
const AUDIT_FILTER: &str = "WHERE (?1 IS NULL OR actor = ?1) AND (?2 IS NULL OR action = ?2)
  AND (?3 IS NULL OR outcome = ?3) AND (?4 IS NULL OR at >= ?4) AND (?5 IS NULL OR at <= ?5)";

/// 行 → 审计记录。
///
/// # Errors
///
/// 列类型不符时返回错误。
fn audit_row(row: &Row<'_>) -> StoreResult<AuditEntry> {
    let detail: String = row.get(7)?;
    Ok(AuditEntry {
        id: row.get(0)?,
        at: row.get(1)?,
        actor: row.get(2)?,
        ip: row.get(3)?,
        action: row.get(4)?,
        target: row.get(5)?,
        outcome: row.get(6)?,
        detail: serde_json::from_str(&detail).unwrap_or(Value::String(detail)),
    })
}

/// 去重列值。
///
/// # Errors
///
/// 查询失败时返回错误。
fn distinct(conn: &Connection, column: &str) -> StoreResult<Vec<String>> {
    query_vec(
        conn,
        &format!("SELECT DISTINCT {column} FROM audit_log ORDER BY {column} LIMIT 200"),
        [],
        |row| Ok(row.get::<_, String>(0)?),
    )
}

/// 审计分页查询。
///
/// # Errors
///
/// 查询失败时返回错误。
pub(crate) fn audit_page(conn: &Connection, query: &AuditQuery) -> StoreResult<AuditPage> {
    let since = query.since.map(ts).transpose()?;
    let until = query.until.map(ts).transpose()?;
    let params = rusqlite::params![query.actor, query.action, query.outcome, since, until];
    let total = to_u64(query_i64(
        conn,
        &format!("SELECT count(*) FROM audit_log {AUDIT_FILTER}"),
        params,
    )?)?;
    let limit = query.limit.clamp(1, AUDIT_PAGE_MAX);
    let entries = query_vec(
        conn,
        &format!(
            "SELECT id, at, actor, ip, action, target, outcome, detail FROM audit_log {AUDIT_FILTER}
             ORDER BY id DESC LIMIT ?6 OFFSET ?7"
        ),
        rusqlite::params![
            query.actor,
            query.action,
            query.outcome,
            since,
            until,
            limit,
            query.offset
        ],
        audit_row,
    )?;
    Ok(AuditPage {
        total,
        entries,
        actors: distinct(conn, "actor")?,
        actions: distinct(conn, "action")?,
    })
}

/// 状态文本 → 枚举。
///
/// # Errors
///
/// 未知状态时返回错误。
fn state(raw: &str) -> StoreResult<NodeState> {
    Ok(serde_json::from_value(Value::String(raw.to_owned()))?)
}

/// 行 → 事件详情。
///
/// # Errors
///
/// 列内容不合法时返回错误。
fn event_row(row: &Row<'_>) -> StoreResult<EventDetail> {
    let payload: String = row.get(10)?;
    Ok(EventDetail {
        id: row.get(0)?,
        node_id: row.get(1)?,
        hostname: row.get(2)?,
        session_id: row.get(3)?,
        event_id: row.get(4)?,
        at: parse_ts(&row.get::<_, String>(5)?)?,
        source: row.get(6)?,
        kind: row.get(7)?,
        from_state: state(&row.get::<_, String>(8)?)?,
        to_state: state(&row.get::<_, String>(9)?)?,
        payload: serde_json::from_str(&payload).unwrap_or(Value::String(payload)),
    })
}

/// 事件详情查询的公共部分。
const SELECT_EVENT: &str =
    "SELECT e.id, e.node_id, n.hostname, e.session_id, e.event_id, e.at, e.source,
  e.kind, e.from_state, e.to_state, e.payload FROM node_events e JOIN nodes n ON n.id = e.node_id";

/// 事件详情：`node_id` 为 `None` 时跨全部节点（从新到旧）。
///
/// # Errors
///
/// 查询失败时返回错误。
pub(crate) fn events(
    conn: &Connection,
    node_id: Option<i64>,
    limit: u32,
) -> StoreResult<Vec<EventDetail>> {
    query_vec(
        conn,
        &format!("{SELECT_EVENT} WHERE (?1 IS NULL OR e.node_id = ?1) ORDER BY e.id DESC LIMIT ?2"),
        rusqlite::params![node_id, limit],
        event_row,
    )
}

/// 某会话的全部事件（从旧到新）。
///
/// # Errors
///
/// 查询失败时返回错误。
pub(crate) fn session_events(conn: &Connection, session: &str) -> StoreResult<Vec<EventDetail>> {
    query_vec(
        conn,
        &format!("{SELECT_EVENT} WHERE e.session_id = ?1 ORDER BY e.id"),
        [session],
        event_row,
    )
}

/// `since` 之后每天到达 `Ready` / `Failed` 的次数（按日期升序，只含有结果的日子）。
///
/// # Errors
///
/// 查询失败时返回错误。
pub(crate) fn outcomes(conn: &Connection, since: Timestamp) -> StoreResult<Vec<DayOutcome>> {
    query_vec(
        conn,
        "SELECT substr(at, 1, 10) AS day,
                sum(CASE WHEN to_state = 'ready' THEN 1 ELSE 0 END),
                sum(CASE WHEN to_state = 'failed' THEN 1 ELSE 0 END)
         FROM node_events WHERE at >= ?1 AND from_state <> to_state AND to_state IN ('ready', 'failed')
         GROUP BY day ORDER BY day",
        [ts(since)?],
        |row| {
            Ok(DayOutcome {
                day: row.get(0)?,
                ready: to_u64(row.get(1)?)?,
                failed: to_u64(row.get(2)?)?,
            })
        },
    )
}
