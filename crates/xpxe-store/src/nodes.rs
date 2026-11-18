//! 节点运行态与事件流（`(node_id, event_id)` 唯一，保证事件幂等）。

use rusqlite::{Connection, Row, Transaction};
use serde::Serialize;
use serde_json::Value;
use xpxe_domain::{EventId, Hostname, MacAddr, NodeState, SessionId, Timestamp};

use crate::sqlval::{exec, parse_opt_ts, parse_ts, query_opt, query_vec, ts};
use crate::{Store, StoreError, StoreResult};

/// 新节点。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewNode {
    /// `node_spec` 配置对象 ID。
    pub spec_object_id: i64,
    /// 引导网卡 MAC。
    pub mac: MacAddr,
    /// 主机名。
    pub hostname: Hostname,
}

/// 节点运行态行。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NodeRow {
    /// 节点 ID。
    pub id: i64,
    /// `node_spec` 配置对象 ID。
    pub spec_object_id: i64,
    /// MAC（冒号小写）。
    pub mac: String,
    /// 主机名。
    pub hostname: String,
    /// 状态。
    pub state: NodeState,
    /// 进入当前状态的时间。
    pub state_since: Timestamp,
    /// 当前状态的截止时间。
    pub deadline_at: Option<Timestamp>,
    /// 当前装机会话。
    pub active_session_id: Option<SessionId>,
    /// 最近一次事件 ID。
    pub last_event_id: Option<String>,
}

/// 一条状态机事件，连同转换后的运行态一起原子写入。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventRecord {
    /// 发送方生成的事件 ID。
    pub event_id: EventId,
    /// 会话。
    pub session_id: Option<SessionId>,
    /// 发生时间。
    pub at: Timestamp,
    /// 来源（`admin`、`installer`、`agent`、`timer` …）。
    pub source: String,
    /// 事件种类。
    pub kind: String,
    /// 转换前状态。
    pub from_state: NodeState,
    /// 转换后状态。
    pub to_state: NodeState,
    /// 截止时间（转换后）。
    pub deadline_at: Option<Timestamp>,
    /// 转换后的活动会话。
    pub active_session_id: Option<SessionId>,
    /// 附加载荷。
    pub payload: Value,
}

/// 事件写入结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventInsert {
    /// 新事件，返回行 ID。
    Inserted(i64),
    /// 同一 `event_id` 已经记录过，运行态未改动。
    Duplicate,
}

/// 节点句柄。
#[derive(Debug, Clone, Copy)]
pub struct Nodes<'store> {
    /// 所属存储。
    store: &'store Store,
}

impl<'store> Nodes<'store> {
    /// 构造句柄。
    pub(crate) const fn new(store: &'store Store) -> Self {
        Self { store }
    }

    /// 登记节点（初始 `Idle`）。
    ///
    /// # Errors
    ///
    /// MAC、主机名或规格对象重复、规格对象不存在时返回错误。
    pub fn create(self, node: &NewNode, now: Timestamp) -> StoreResult<NodeRow> {
        self.store.write(|tx| {
            exec(tx,
                "INSERT INTO nodes (spec_object_id, mac, hostname, state, state_since) VALUES (?1, ?2, ?3, 'idle', ?4)",
                rusqlite::params![
                    node.spec_object_id,
                    node.mac.to_colon(),
                    node.hostname.as_str(),
                    ts(now)?
                ],
            )?;
            node_by(tx, "id = ?1", tx.last_insert_rowid())
        })
    }

    /// 按 ID 读取。
    ///
    /// # Errors
    ///
    /// 节点不存在时返回错误。
    pub fn get(self, node_id: i64) -> StoreResult<NodeRow> {
        self.store.read(|conn| node_by(conn, "id = ?1", node_id))
    }

    /// 按 MAC 读取。
    ///
    /// # Errors
    ///
    /// 查询失败时返回错误。
    pub fn by_mac(self, mac: MacAddr) -> StoreResult<Option<NodeRow>> {
        self.store
            .read(|conn| match node_by(conn, "mac = ?1", mac.to_colon()) {
                Ok(row) => Ok(Some(row)),
                Err(StoreError::NotFound(_)) => Ok(None),
                Err(error) => Err(error),
            })
    }

    /// 全部节点（按主机名）。
    ///
    /// # Errors
    ///
    /// 查询失败时返回错误。
    pub fn list(self) -> StoreResult<Vec<NodeRow>> {
        self.store.read(|conn| {
            query_vec(
                conn,
                &format!("{SELECT_NODE} ORDER BY hostname"),
                [],
                node_row,
            )
        })
    }

    /// 原子地记录事件并更新运行态；重复的 `event_id` 不改动任何东西。
    ///
    /// # Errors
    ///
    /// 节点当前状态与 `from_state` 不符时返回 [`StoreError::StateChanged`]；
    /// 写入失败时返回错误。
    pub fn record(self, node_id: i64, event: &EventRecord) -> StoreResult<EventInsert> {
        self.store.write(|tx| record_event(tx, node_id, event))
    }

    /// 最近的事件（从新到旧），返回 `(event_id, kind, from, to, at)`。
    ///
    /// # Errors
    ///
    /// 查询失败时返回错误。
    pub fn events(self, node_id: i64, limit: u32) -> StoreResult<Vec<EventSummary>> {
        self.store.read(|conn| {
            query_vec(
                conn,
                "SELECT event_id, kind, from_state, to_state, at FROM node_events
                 WHERE node_id = ?1 ORDER BY id DESC LIMIT ?2",
                rusqlite::params![node_id, limit],
                |row| {
                    Ok(EventSummary {
                        event_id: row.get(0)?,
                        kind: row.get(1)?,
                        from_state: parse_state(&row.get::<_, String>(2)?)?,
                        to_state: parse_state(&row.get::<_, String>(3)?)?,
                        at: parse_ts(&row.get::<_, String>(4)?)?,
                    })
                },
            )
        })
    }
}

/// 事件摘要。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EventSummary {
    /// 事件 ID。
    pub event_id: String,
    /// 种类。
    pub kind: String,
    /// 转换前状态。
    pub from_state: NodeState,
    /// 转换后状态。
    pub to_state: NodeState,
    /// 时间。
    pub at: Timestamp,
}

/// 节点查询的公共列。
const SELECT_NODE: &str =
    "SELECT id, spec_object_id, mac, hostname, state, state_since, deadline_at,
  active_session_id, last_event_id FROM nodes";

/// 行 → 节点。
///
/// # Errors
///
/// 列内容不合法时返回错误。
fn node_row(row: &Row<'_>) -> StoreResult<NodeRow> {
    let session: Option<String> = row.get(7)?;
    Ok(NodeRow {
        id: row.get(0)?,
        spec_object_id: row.get(1)?,
        mac: row.get(2)?,
        hostname: row.get(3)?,
        state: parse_state(&row.get::<_, String>(4)?)?,
        state_since: parse_ts(&row.get::<_, String>(5)?)?,
        deadline_at: parse_opt_ts(row.get::<_, Option<String>>(6)?.as_deref())?,
        active_session_id: session
            .as_deref()
            .map(SessionId::parse)
            .transpose()
            .map_err(|error| StoreError::Corrupt(error.to_string()))?,
        last_event_id: row.get(8)?,
    })
}

/// 按条件读取一个节点。
///
/// # Errors
///
/// 不存在时返回 [`StoreError::NotFound`]。
fn node_by<P: rusqlite::ToSql>(conn: &Connection, filter: &str, param: P) -> StoreResult<NodeRow> {
    query_opt(
        conn,
        &format!("{SELECT_NODE} WHERE {filter}"),
        [param],
        node_row,
    )?
    .ok_or(StoreError::NotFound("节点"))
}

/// 状态文本 → 枚举。
///
/// # Errors
///
/// 未知状态时返回错误。
fn parse_state(raw: &str) -> StoreResult<NodeState> {
    Ok(serde_json::from_value(Value::String(raw.to_owned()))?)
}

/// 事务内写事件与运行态。
///
/// # Errors
///
/// 状态不符或写入失败时返回错误。
fn record_event(
    tx: &Transaction<'_>,
    node_id: i64,
    event: &EventRecord,
) -> StoreResult<EventInsert> {
    let seen = query_opt(
        tx,
        "SELECT id FROM node_events WHERE node_id = ?1 AND event_id = ?2",
        rusqlite::params![node_id, event.event_id.as_str()],
        |row| Ok(row.get::<_, i64>(0)?),
    )?;
    if seen.is_some() {
        return Ok(EventInsert::Duplicate);
    }
    let current = node_by(tx, "id = ?1", node_id)?;
    if current.state != event.from_state {
        return Err(StoreError::StateChanged {
            current: current.state,
        });
    }
    exec(tx,
        "INSERT INTO node_events (node_id, session_id, event_id, at, source, kind, from_state, to_state, payload)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        rusqlite::params![
            node_id,
            event.session_id.as_ref().map(SessionId::as_str),
            event.event_id.as_str(),
            ts(event.at)?,
            event.source,
            event.kind,
            event.from_state.as_str(),
            event.to_state.as_str(),
            serde_json::to_string(&event.payload)?,
        ],
    )?;
    let row_id = tx.last_insert_rowid();
    let since = if event.to_state == event.from_state {
        current.state_since
    } else {
        event.at
    };
    exec(
        tx,
        "UPDATE nodes SET state = ?2, state_since = ?3, deadline_at = ?4, active_session_id = ?5,
           last_event_id = ?6 WHERE id = ?1",
        rusqlite::params![
            node_id,
            event.to_state.as_str(),
            ts(since)?,
            event.deadline_at.map(ts).transpose()?,
            event.active_session_id.as_ref().map(SessionId::as_str),
            event.event_id.as_str(),
        ],
    )?;
    Ok(EventInsert::Inserted(row_id))
}
