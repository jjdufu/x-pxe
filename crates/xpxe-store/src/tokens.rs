//! 装机会话与令牌：32 字节 CSPRNG、base64url 无填充，库里只存 SHA-256；
//! `Boot` 与 `Agent` 令牌分别只能访问各自的资源；preseed 与计划只下发一次。

use std::time::Duration;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use xpxe_domain::{BootToken, SessionId, Timestamp};

use crate::config::VersionRef;
use crate::sqlval::{
    ct_eq, exec, parse_opt_ts, parse_ts, query_opt, query_vec, random, random_token, sha256_hex, ts,
};
use crate::{Store, StoreError, StoreResult, expired};

/// 令牌种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TokenKind {
    /// 装机阶段：iPXE 脚本、内核、initrd、preseed、ISO、仓库公钥。
    Boot,
    /// 首次启动阶段：agent、计划、事件。
    Agent,
}

/// 令牌可访问的资源。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Resource {
    /// iPXE 脚本。
    IpxeScript,
    /// 内核。
    Kernel,
    /// initrd。
    Initrd,
    /// preseed。
    Preseed,
    /// ISO。
    Iso,
    /// 仓库公钥。
    RepoKey,
    /// agent 二进制。
    Agent,
    /// 初始化计划。
    Plan,
    /// 事件上报。
    Events,
}

impl TokenKind {
    /// 此种类令牌是否可访问 `resource`。
    #[must_use]
    pub const fn allows(self, resource: Resource) -> bool {
        match self {
            Self::Boot => matches!(
                resource,
                Resource::IpxeScript
                    | Resource::Kernel
                    | Resource::Initrd
                    | Resource::Preseed
                    | Resource::Iso
                    | Resource::RepoKey
            ),
            Self::Agent => matches!(
                resource,
                Resource::Agent | Resource::Plan | Resource::Events
            ),
        }
    }
}

/// 一次性下发的文档。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ServedDoc {
    /// preseed。
    Preseed,
    /// 初始化计划。
    Plan,
}

/// 首次启动阶段令牌。
#[derive(Clone, PartialEq, Eq)]
pub struct AgentToken(String);

impl AgentToken {
    /// 令牌文本。
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for AgentToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AgentToken(***)")
    }
}

/// 装机时固定的配置版本。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pinned {
    /// Profile 版本。
    pub profile: VersionRef,
    /// 初始化计划版本。
    pub init_plan: Option<VersionRef>,
    /// 资产频道版本。
    pub asset_channels: Option<VersionRef>,
    /// 金丝雀会话以候选资产覆盖频道（资产 ID）；普通会话为空。
    #[serde(default)]
    pub canary_assets: Vec<i64>,
}

/// 新建装机会话的参数。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRequest {
    /// 节点 ID。
    pub node_id: i64,
    /// 引导通道（`lan` / `redfish`）。
    pub channel: String,
    /// 固定的配置版本。
    pub pinned: Pinned,
    /// 有效期。
    pub ttl: Duration,
}

/// 新签发的会话与令牌原值（只在此时可见）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssuedSession {
    /// 会话 ID。
    pub session: SessionId,
    /// 装机令牌。
    pub boot: BootToken,
    /// 首次启动令牌。
    pub agent: AgentToken,
    /// 过期时间。
    pub expires_at: Timestamp,
}

/// 装机会话记录。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallSession {
    /// 会话 ID。
    pub id: SessionId,
    /// 节点 ID。
    pub node_id: i64,
    /// 引导通道。
    pub channel: String,
    /// 过期时间。
    pub expires_at: Timestamp,
    /// 关闭时间。
    pub closed_at: Option<Timestamp>,
    /// preseed 下发时间。
    pub preseed_served_at: Option<Timestamp>,
    /// 计划下发时间。
    pub plan_served_at: Option<Timestamp>,
    /// 固定的版本。
    pub pinned: Pinned,
    /// 结果。
    pub outcome: Option<String>,
}

/// 装机令牌句柄。
#[derive(Debug, Clone, Copy)]
pub struct Tokens<'store> {
    /// 所属存储。
    store: &'store Store,
}

impl<'store> Tokens<'store> {
    /// 构造句柄。
    pub(crate) const fn new(store: &'store Store) -> Self {
        Self { store }
    }

    /// 新建装机会话并签发两种令牌。
    ///
    /// # Errors
    ///
    /// 节点不存在、随机源或写入失败时返回错误。
    pub fn issue(self, request: &SessionRequest, now: Timestamp) -> StoreResult<IssuedSession> {
        let session = SessionId::parse(&hex::encode(random::<16>()?))
            .map_err(|error| StoreError::Invalid(error.to_string()))?;
        let boot = BootToken::parse(&random_token()?)
            .map_err(|error| StoreError::Invalid(error.to_string()))?;
        let agent = AgentToken(random_token()?);
        let expires_at = now.saturating_add(request.ttl);
        self.store.write(|tx| {
            exec(tx,
                "INSERT INTO install_sessions (id, node_id, channel, boot_token_hash, agent_token_hash,
                   created_at, expires_at, pinned) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![
                    session.as_str(),
                    request.node_id,
                    request.channel,
                    sha256_hex(boot.as_str().as_bytes()),
                    sha256_hex(agent.as_str().as_bytes()),
                    ts(now)?,
                    ts(expires_at)?,
                    serde_json::to_string(&request.pinned)?,
                ],
            )?;
            Ok(())
        })?;
        Ok(IssuedSession {
            session,
            boot,
            agent,
            expires_at,
        })
    }

    /// 解析令牌：只接受未过期、未关闭会话的令牌，常量时间比较哈希。
    ///
    /// # Errors
    ///
    /// 查询失败时返回错误。
    pub fn resolve(
        self,
        token: &str,
        now: Timestamp,
    ) -> StoreResult<Option<(SessionId, TokenKind)>> {
        let hash = sha256_hex(token.as_bytes());
        self.store.read(|conn| {
            let row = query_opt(
                conn,
                "SELECT id, boot_token_hash, agent_token_hash, expires_at, closed_at FROM install_sessions
                 WHERE boot_token_hash = ?1 OR agent_token_hash = ?1",
                [&hash],
                |row| {
                    Ok(TokenRow {
                        id: row.get(0)?,
                        boot_hash: row.get(1)?,
                        agent_hash: row.get(2)?,
                        expires_at: parse_ts(&row.get::<_, String>(3)?)?,
                        closed: row.get::<_, Option<String>>(4)?.is_some(),
                    })
                },
            )?;
            let Some(found) = row else {
                return Ok(None);
            };
            if found.closed || expired(found.expires_at, now) {
                return Ok(None);
            }
            let boot_match = ct_eq(found.boot_hash.as_bytes(), hash.as_bytes());
            let agent_match = ct_eq(found.agent_hash.as_bytes(), hash.as_bytes());
            let kind = match (boot_match, agent_match) {
                (true, false) => TokenKind::Boot,
                (false, true) => TokenKind::Agent,
                (true, true) | (false, false) => return Ok(None),
            };
            let session =
                SessionId::parse(&found.id).map_err(|error| StoreError::Corrupt(error.to_string()))?;
            Ok(Some((session, kind)))
        })
    }

    /// 解析令牌并检查其种类能否访问 `resource`。
    ///
    /// # Errors
    ///
    /// 查询失败时返回错误。
    pub fn authorize(
        self,
        token: &str,
        resource: Resource,
        now: Timestamp,
    ) -> StoreResult<Option<SessionId>> {
        Ok(self
            .resolve(token, now)?
            .and_then(|(session, kind)| kind.allows(resource).then_some(session)))
    }

    /// 原子地标记一次性文档已下发；只有第一次调用成功。
    ///
    /// # Errors
    ///
    /// 已经下发过时返回 [`StoreError::AlreadyServed`]。
    pub fn consume_once(
        self,
        session: &SessionId,
        doc: ServedDoc,
        now: Timestamp,
    ) -> StoreResult<()> {
        let sql = match doc {
            ServedDoc::Preseed => {
                "UPDATE install_sessions SET preseed_served_at = ?2 WHERE id = ?1 AND preseed_served_at IS NULL"
            }
            ServedDoc::Plan => {
                "UPDATE install_sessions SET plan_served_at = ?2 WHERE id = ?1 AND plan_served_at IS NULL"
            }
        };
        let changed = self
            .store
            .write(|tx| Ok(tx.execute(sql, [session.as_str(), ts(now)?.as_str()])?))?;
        if changed == 1 {
            Ok(())
        } else {
            Err(StoreError::AlreadyServed)
        }
    }

    /// 关闭会话并记录结果；关闭后令牌失效。
    ///
    /// # Errors
    ///
    /// 写入失败时返回错误。
    pub fn close(self, session: &SessionId, outcome: &str, now: Timestamp) -> StoreResult<bool> {
        self.store.write(|tx| {
            Ok(tx.execute(
                "UPDATE install_sessions SET closed_at = ?2, outcome = ?3 WHERE id = ?1 AND closed_at IS NULL",
                [session.as_str(), ts(now)?.as_str(), outcome],
            )? == 1)
        })
    }

    /// 未关闭且未过期的会话所固定的版本（删除资产前检查引用）。
    ///
    /// # Errors
    ///
    /// 查询失败或固定内容损坏时返回错误。
    pub fn open_pins(self, now: Timestamp) -> StoreResult<Vec<Pinned>> {
        let rows = self.store.read(|conn| {
            query_vec(
                conn,
                "SELECT pinned, expires_at FROM install_sessions WHERE closed_at IS NULL",
                [],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        parse_ts(&row.get::<_, String>(1)?)?,
                    ))
                },
            )
        })?;
        let mut pins = Vec::with_capacity(rows.len());
        for (text, expires_at) in rows {
            if !expired(expires_at, now) {
                pins.push(serde_json::from_str(&text)?);
            }
        }
        Ok(pins)
    }

    /// 读取会话记录。
    ///
    /// # Errors
    ///
    /// 会话不存在时返回错误。
    pub fn get(self, session: &SessionId) -> StoreResult<InstallSession> {
        self.store.read(|conn| session_by_id(conn, session))
    }
}

/// 令牌查询行。
#[derive(Debug)]
struct TokenRow {
    /// 会话 ID。
    id: String,
    /// 装机令牌哈希。
    boot_hash: String,
    /// 首次启动令牌哈希。
    agent_hash: String,
    /// 过期时间。
    expires_at: Timestamp,
    /// 是否已关闭。
    closed: bool,
}

/// 读取会话。
///
/// # Errors
///
/// 不存在或列内容不合法时返回错误。
fn session_by_id(conn: &Connection, session: &SessionId) -> StoreResult<InstallSession> {
    query_opt(
        conn,
        "SELECT node_id, channel, expires_at, closed_at, preseed_served_at, plan_served_at, pinned, outcome
         FROM install_sessions WHERE id = ?1",
        [session.as_str()],
        |row| {
            Ok(InstallSession {
                id: session.clone(),
                node_id: row.get(0)?,
                channel: row.get(1)?,
                expires_at: parse_ts(&row.get::<_, String>(2)?)?,
                closed_at: parse_opt_ts(row.get::<_, Option<String>>(3)?.as_deref())?,
                preseed_served_at: parse_opt_ts(row.get::<_, Option<String>>(4)?.as_deref())?,
                plan_served_at: parse_opt_ts(row.get::<_, Option<String>>(5)?.as_deref())?,
                pinned: serde_json::from_str(&row.get::<_, String>(6)?)?,
                outcome: row.get(7)?,
            })
        },
    )?
    .ok_or(StoreError::NotFound("装机会话"))
}
