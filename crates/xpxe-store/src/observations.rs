//! bootnet 观测（DHCP/TFTP），按时间滚动保留 30 天，支持“发现未登记 MAC”。

use std::time::Duration;

use serde::Serialize;
use xpxe_domain::Timestamp;

use crate::sqlval::{exec, parse_ts, query_vec, ts};
use crate::{Store, StoreResult};

/// 保留时长。
pub const RETENTION: Duration = Duration::from_hours(30 * 24);

/// 一条观测。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Observation {
    /// 时间。
    pub at: Timestamp,
    /// 客户端 MAC（冒号小写）。
    pub mac: String,
    /// 架构（option 93 名称）。
    pub arch: String,
    /// user-class（option 77）。
    pub user_class: String,
    /// 种类（`dhcp_offer`、`dhcp_ignore`、`tftp_read` …）。
    pub kind: String,
    /// 详情。
    pub detail: String,
}

/// 观测句柄。
#[derive(Debug, Clone, Copy)]
pub struct Observations<'store> {
    /// 所属存储。
    store: &'store Store,
}

impl<'store> Observations<'store> {
    /// 构造句柄。
    pub(crate) const fn new(store: &'store Store) -> Self {
        Self { store }
    }

    /// 追加一条观测。
    ///
    /// # Errors
    ///
    /// 写入失败时返回错误。
    pub fn record(self, observation: &Observation) -> StoreResult<()> {
        self.store.write(|tx| {
            exec(tx,
                "INSERT INTO bootnet_observations (at, mac, arch, user_class, kind, detail) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![
                    ts(observation.at)?,
                    observation.mac,
                    observation.arch,
                    observation.user_class,
                    observation.kind,
                    observation.detail
                ],
            )?;
            Ok(())
        })
    }

    /// 删除超过保留期的观测，返回删除条数。
    ///
    /// # Errors
    ///
    /// 写入失败时返回错误。
    pub fn prune(self, now: Timestamp) -> StoreResult<usize> {
        let cutoff =
            Timestamp::from_unix_seconds(now.unix_seconds().saturating_sub(RETENTION.as_secs()));
        self.store.write(|tx| {
            Ok(tx.execute(
                "DELETE FROM bootnet_observations WHERE at < ?1",
                [ts(cutoff)?],
            )?)
        })
    }

    /// 最近的观测（新到旧）。
    ///
    /// # Errors
    ///
    /// 查询失败时返回错误。
    pub fn recent(self, limit: u32) -> StoreResult<Vec<Observation>> {
        self.store.read(|conn| {
            query_vec(
                conn,
                "SELECT at, mac, arch, user_class, kind, detail FROM bootnet_observations ORDER BY id DESC LIMIT ?1",
                [limit],
                |row| {
                    Ok(Observation {
                        at: parse_ts(&row.get::<_, String>(0)?)?,
                        mac: row.get(1)?,
                        arch: row.get(2)?,
                        user_class: row.get(3)?,
                        kind: row.get(4)?,
                        detail: row.get(5)?,
                    })
                },
            )
        })
    }

    /// 观测到但未登记为节点的 MAC（去重，按最近出现排序）。
    ///
    /// # Errors
    ///
    /// 查询失败时返回错误。
    pub fn unknown_macs(self) -> StoreResult<Vec<(String, Timestamp)>> {
        self.store.read(|conn| {
            query_vec(
                conn,
                "SELECT mac, max(at) AS last FROM bootnet_observations
                 WHERE mac NOT IN (SELECT mac FROM nodes) GROUP BY mac ORDER BY last DESC",
                [],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        parse_ts(&row.get::<_, String>(1)?)?,
                    ))
                },
            )
        })
    }
}
