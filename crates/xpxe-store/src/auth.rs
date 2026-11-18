//! 账户：argon2id 口令、Cookie 会话（只存哈希）、API Token 与登录退避。

use std::str::FromStr;
use std::time::Duration;

use argon2::password_hash::phc::PasswordHash;
use argon2::password_hash::{PasswordHasher as _, PasswordVerifier as _};
use argon2::{Algorithm, Argon2, Params, Version};
use rusqlite::{Connection, Row, Transaction};
use serde::Serialize;
use xpxe_domain::Timestamp;

use crate::sqlval::{
    ct_eq, exec, parse_ts, query_i64, query_opt, random, random_token, sha256_hex, to_u64, ts,
};
use crate::{Store, StoreError, StoreResult, expired};

/// argon2id 内存成本（KiB）：64 MiB。
pub const ARGON2_M_KIB: u32 = 65_536;
/// argon2id 迭代次数。
pub const ARGON2_T: u32 = 3;
/// argon2id 并行度。
pub const ARGON2_P: u32 = 1;
/// 口令最短字符数。
pub const PASSWORD_MIN: usize = 12;
/// 口令最长字符数。
pub const PASSWORD_MAX: usize = 256;
/// 会话空闲超时。
pub const SESSION_IDLE: Duration = Duration::from_mins(30);
/// 会话绝对超时。
pub const SESSION_ABSOLUTE: Duration = Duration::from_hours(12);
/// API Token 最长有效期。
pub const TOKEN_MAX_TTL: Duration = Duration::from_hours(365 * 24);
/// API Token 前缀。
pub const TOKEN_PREFIX: &str = "xpxe_";
/// 退避上限（秒）。
pub const BACKOFF_CAP_SECS: u64 = 900;

/// 账户角色。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// 管理员。
    Admin,
    /// 操作员。
    Operator,
    /// 只读。
    Viewer,
}

impl Role {
    /// 存储名称。
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Admin => "admin",
            Self::Operator => "operator",
            Self::Viewer => "viewer",
        }
    }
}

impl FromStr for Role {
    type Err = StoreError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "admin" => Ok(Self::Admin),
            "operator" => Ok(Self::Operator),
            "viewer" => Ok(Self::Viewer),
            other => Err(StoreError::Invalid(format!("未知角色 {other}"))),
        }
    }
}

/// API Token 作用域。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    /// 只读。
    Read,
    /// 节点操作。
    Operate,
    /// 管理。
    Admin,
}

impl Scope {
    /// 存储名称。
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Operate => "operate",
            Self::Admin => "admin",
        }
    }
}

impl FromStr for Scope {
    type Err = StoreError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "read" => Ok(Self::Read),
            "operate" => Ok(Self::Operate),
            "admin" => Ok(Self::Admin),
            other => Err(StoreError::Invalid(format!("未知作用域 {other}"))),
        }
    }
}

/// 管理端账户。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct User {
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
}

/// 一次登录尝试。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginAttempt {
    /// 用户名。
    pub username: String,
    /// 口令。
    pub password: String,
    /// 来源 IP。
    pub ip: String,
    /// User-Agent。
    pub user_agent: String,
}

/// 新建会话的原值（只在这一次返回，库里只有哈希）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewSession {
    /// Cookie 值。
    pub session_id: String,
    /// CSRF 令牌。
    pub csrf_token: String,
    /// 绝对过期时间。
    pub expires_at: Timestamp,
}

/// 有效会话对应的用户。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionUser {
    /// 用户。
    pub user: User,
    /// 会话 ID 的哈希。
    pub session_hash: String,
    /// CSRF 令牌的哈希。
    pub csrf_hash: String,
}

impl SessionUser {
    /// 常量时间核对 CSRF 令牌。
    #[must_use]
    pub fn csrf_matches(&self, csrf_token: &str) -> bool {
        ct_eq(
            sha256_hex(csrf_token.as_bytes()).as_bytes(),
            self.csrf_hash.as_bytes(),
        )
    }
}

/// 申请 API Token 的参数。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenGrant {
    /// 名称。
    pub name: String,
    /// 作用域（非空）。
    pub scopes: Vec<Scope>,
    /// 过期时间（必须设置，≤ 365 天）。
    pub expires_at: Timestamp,
}

/// API Token 解析结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiPrincipal {
    /// Token ID。
    pub token_id: i64,
    /// 所属用户。
    pub user: User,
    /// 作用域。
    pub scopes: Vec<Scope>,
}

/// 连续失败 `failures` 次之后的退避秒数：前 4 次不退避，第 5 次起为 2ⁿ 秒
/// （n = failures − 4，即 2、4、8 …），上限 15 min。
#[must_use]
pub fn backoff_delay(failures: u64) -> u64 {
    let Some(exponent) = failures.checked_sub(4).filter(|exp| *exp > 0) else {
        return 0;
    };
    u32::try_from(exponent)
        .ok()
        .and_then(|exp| 2_u64.checked_pow(exp))
        .map_or(BACKOFF_CAP_SECS, |delay| delay.min(BACKOFF_CAP_SECS))
}

/// 账户句柄。
#[derive(Debug, Clone, Copy)]
pub struct Accounts<'store> {
    /// 所属存储。
    store: &'store Store,
}

impl<'store> Accounts<'store> {
    /// 构造句柄。
    pub(crate) const fn new(store: &'store Store) -> Self {
        Self { store }
    }

    /// `users` 为空时用口令文件创建 `admin`（`must_change_password = true`）。
    ///
    /// 返回是否创建了管理员。
    ///
    /// # Errors
    ///
    /// 库为空但口令文件缺失、口令不合规或写入失败时返回错误。
    pub fn ensure_initial_admin(
        self,
        password_file: Option<&std::path::Path>,
        now: Timestamp,
    ) -> StoreResult<bool> {
        let count = self
            .store
            .read(|conn| query_i64(conn, "SELECT count(*) FROM users", []))?;
        if count > 0 {
            return Ok(false);
        }
        let path = password_file.ok_or_else(|| {
            StoreError::Invalid("库中没有用户，需要 XPXE_ADMIN_INITIAL_PASSWORD_FILE".to_owned())
        })?;
        let raw = std::fs::read_to_string(path)?;
        let password = raw.trim_end_matches(['\r', '\n']);
        let admin = self.create_user("admin", password, Role::Admin, now)?;
        self.store.write(|tx| {
            exec(
                tx,
                "UPDATE users SET must_change_password = 1 WHERE id = ?1",
                [admin.id],
            )?;
            Ok(())
        })?;
        Ok(true)
    }

    /// 新建用户。
    ///
    /// # Errors
    ///
    /// 用户名或口令不合规、用户名重复或写入失败时返回错误。
    pub fn create_user(
        self,
        username: &str,
        password: &str,
        role: Role,
        now: Timestamp,
    ) -> StoreResult<User> {
        check_username(username)?;
        let hash = hash_password(password)?;
        self.store.write(|tx| {
            exec(tx,
                "INSERT INTO users (username, password_hash, role, created_at) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![username, hash, role.as_str(), ts(now)?],
            )?;
            user_by_id(tx, tx.last_insert_rowid())
        })
    }

    /// 按 ID 读取用户。
    ///
    /// # Errors
    ///
    /// 用户不存在时返回错误。
    pub fn user(self, user_id: i64) -> StoreResult<User> {
        self.store.read(|conn| user_by_id(conn, user_id))
    }

    /// 登录：先查退避，再核对口令；成功清零计数并建立会话。
    ///
    /// # Errors
    ///
    /// 退避期间返回 [`StoreError::Throttled`]；用户不存在、已禁用或口令错误统一返回
    /// [`StoreError::InvalidCredentials`]。
    pub fn login(self, attempt: &LoginAttempt, now: Timestamp) -> StoreResult<(User, NewSession)> {
        let retry_after = self.store.read(|conn| {
            let by_user = throttle_remaining(conn, "user", &attempt.username, now)?;
            let by_ip = throttle_remaining(conn, "ip", &attempt.ip, now)?;
            Ok(by_user.max(by_ip))
        })?;
        if retry_after > 0 {
            return Err(StoreError::Throttled { retry_after });
        }
        let found = self.store.read(|conn| {
            query_opt(
                conn,
                "SELECT id, password_hash FROM users WHERE username = ?1",
                [&attempt.username],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
            )
        })?;
        let verified = match &found {
            Some((id, stored)) => verify_password(&attempt.password, stored)?.then_some(*id),
            None => {
                // 用户不存在也做一次同成本哈希，避免时间侧信道暴露用户是否存在。
                hash_password(&attempt.password).map(|_hash| None)?
            }
        };
        let active = match verified {
            Some(id) => Some(self.user(id)?).filter(|candidate| !candidate.disabled),
            None => None,
        };
        let Some(user) = active else {
            self.store.write(|tx| {
                record_failure(tx, "user", &attempt.username, now)?;
                record_failure(tx, "ip", &attempt.ip, now)
            })?;
            return Err(StoreError::InvalidCredentials);
        };
        let session = self.store.write(|tx| {
            exec(
                tx,
                "DELETE FROM login_failures WHERE (scope = 'user' AND subject = ?1) OR (scope = 'ip' AND subject = ?2)",
                [&attempt.username, &attempt.ip],
            )?;
            insert_session(tx, user.id, attempt, now)
        })?;
        Ok((user, session))
    }

    /// 校验会话：检查空闲 30 min 与绝对 12 h，有效时刷新 `last_seen_at`。
    ///
    /// # Errors
    ///
    /// 查询或写入失败时返回错误。
    pub fn session(self, session_id: &str, now: Timestamp) -> StoreResult<Option<SessionUser>> {
        let id_hash = sha256_hex(session_id.as_bytes());
        self.store.write(|tx| {
            let Some(found) = query_opt(
                tx,
                "SELECT user_id, csrf_hash, expires_at, last_seen_at FROM sessions WHERE id_hash = ?1",
                [&id_hash],
                session_row,
            )?
            else {
                return Ok(None);
            };
            let idle_deadline = found.last_seen_at.saturating_add(SESSION_IDLE);
            let user = user_by_id(tx, found.user_id)?;
            if expired(found.expires_at, now) || expired(idle_deadline, now) || user.disabled {
                exec(tx, "DELETE FROM sessions WHERE id_hash = ?1", [&id_hash])?;
                return Ok(None);
            }
            exec(
                tx,
                "UPDATE sessions SET last_seen_at = ?2 WHERE id_hash = ?1",
                [&id_hash, &ts(now)?],
            )?;
            Ok(Some(SessionUser {
                user,
                session_hash: id_hash.clone(),
                csrf_hash: found.csrf_hash,
            }))
        })
    }

    /// 注销一个会话。
    ///
    /// # Errors
    ///
    /// 写入失败时返回错误。
    pub fn logout(self, session_id: &str) -> StoreResult<bool> {
        let id_hash = sha256_hex(session_id.as_bytes());
        self.store
            .write(|tx| Ok(tx.execute("DELETE FROM sessions WHERE id_hash = ?1", [&id_hash])? > 0))
    }

    /// 某用户的有效会话数。
    ///
    /// # Errors
    ///
    /// 查询失败时返回错误。
    pub fn session_count(self, user_id: i64) -> StoreResult<u64> {
        self.store.read(|conn| {
            to_u64(query_i64(
                conn,
                "SELECT count(*) FROM sessions WHERE user_id = ?1",
                [user_id],
            )?)
        })
    }

    /// 改密：核对旧口令，写入新口令并注销该用户除 `keep_session` 以外的全部会话。
    ///
    /// # Errors
    ///
    /// 旧口令错误返回 [`StoreError::InvalidCredentials`]；新口令不合规或写入失败时返回错误。
    pub fn change_password(
        self,
        user_id: i64,
        old: &str,
        new: &str,
        keep_session: Option<&str>,
    ) -> StoreResult<()> {
        let stored = self
            .store
            .read(|conn| {
                query_opt(
                    conn,
                    "SELECT password_hash FROM users WHERE id = ?1",
                    [user_id],
                    |row| Ok(row.get::<_, String>(0)?),
                )
            })?
            .ok_or(StoreError::NotFound("用户"))?;
        if !verify_password(old, &stored)? {
            return Err(StoreError::InvalidCredentials);
        }
        let hash = hash_password(new)?;
        let keep = keep_session.map(|raw| sha256_hex(raw.as_bytes()));
        self.store.write(|tx| {
            exec(
                tx,
                "UPDATE users SET password_hash = ?2, must_change_password = 0 WHERE id = ?1",
                rusqlite::params![user_id, hash],
            )?;
            exec(
                tx,
                "DELETE FROM sessions WHERE user_id = ?1 AND id_hash IS NOT ?2",
                rusqlite::params![user_id, keep],
            )
        })
    }

    /// 签发 API Token，返回 `(token_id, 原值)`；原值只在此时可见。
    ///
    /// # Errors
    ///
    /// 作用域为空、过期时间不在 `(now, now + 365 天]` 内或写入失败时返回错误。
    pub fn issue_api_token(
        self,
        user_id: i64,
        grant: &TokenGrant,
        now: Timestamp,
    ) -> StoreResult<(i64, String)> {
        if grant.scopes.is_empty() {
            return Err(StoreError::Invalid("作用域不能为空".to_owned()));
        }
        if expired(grant.expires_at, now) || grant.expires_at > now.saturating_add(TOKEN_MAX_TTL) {
            return Err(StoreError::Invalid("过期时间必须在 365 天以内".to_owned()));
        }
        let raw = format!("{TOKEN_PREFIX}{}", random_token()?);
        let scopes = grant
            .scopes
            .iter()
            .map(|scope| scope.as_str())
            .collect::<Vec<_>>()
            .join(",");
        let id = self.store.write(|tx| {
            exec(
                tx,
                "INSERT INTO api_tokens (token_hash, user_id, name, scopes, created_at, expires_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![
                    sha256_hex(raw.as_bytes()),
                    user_id,
                    grant.name,
                    scopes,
                    ts(now)?,
                    ts(grant.expires_at)?
                ],
            )?;
            Ok(tx.last_insert_rowid())
        })?;
        Ok((id, raw))
    }

    /// 解析 `Authorization: Bearer` 中的 API Token。
    ///
    /// # Errors
    ///
    /// 查询或写入失败时返回错误。
    pub fn api_token(self, raw: &str, now: Timestamp) -> StoreResult<Option<ApiPrincipal>> {
        if !raw.starts_with(TOKEN_PREFIX) {
            return Ok(None);
        }
        let hash = sha256_hex(raw.as_bytes());
        self.store.write(|tx| {
            let Some(found) = query_opt(
                tx,
                "SELECT id, token_hash, user_id, scopes, expires_at FROM api_tokens WHERE token_hash = ?1",
                [&hash],
                token_row,
            )?
            else {
                return Ok(None);
            };
            let owner = user_by_id(tx, found.user_id)?;
            if !ct_eq(found.stored.as_bytes(), hash.as_bytes())
                || expired(found.expires_at, now)
                || owner.disabled
            {
                return Ok(None);
            }
            exec(
                tx,
                "UPDATE api_tokens SET last_used_at = ?2 WHERE id = ?1",
                rusqlite::params![found.token_id, ts(now)?],
            )?;
            let scopes = found
                .scopes
                .split(',')
                .map(Scope::from_str)
                .collect::<StoreResult<Vec<_>>>()?;
            Ok(Some(ApiPrincipal {
                token_id: found.token_id,
                user: owner,
                scopes,
            }))
        })
    }

    /// 吊销某用户的一个 API Token。
    ///
    /// # Errors
    ///
    /// 写入失败时返回错误。
    pub fn revoke_api_token(self, user_id: i64, token_id: i64) -> StoreResult<bool> {
        self.store.write(|tx| {
            Ok(tx.execute(
                "DELETE FROM api_tokens WHERE id = ?1 AND user_id = ?2",
                [token_id, user_id],
            )? > 0)
        })
    }
}

/// 用户名规则：1–64 位小写字母、数字、`.`、`_`、`-`。
///
/// # Errors
///
/// 不合规时返回错误。
fn check_username(username: &str) -> StoreResult<()> {
    let ok = !username.is_empty()
        && username.len() <= 64
        && username.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
        });
    if ok {
        Ok(())
    } else {
        Err(StoreError::Invalid(
            "用户名必须为 1–64 位小写字母、数字、._-".to_owned(),
        ))
    }
}

/// 生产参数的 argon2id 实例。
///
/// # Errors
///
/// 参数非法时返回错误（不会发生）。
fn argon2() -> StoreResult<Argon2<'static>> {
    let params = Params::new(ARGON2_M_KIB, ARGON2_T, ARGON2_P, None)
        .map_err(|error| StoreError::Invalid(error.to_string()))?;
    Ok(Argon2::new(Algorithm::Argon2id, Version::V0x13, params))
}

/// 校验口令长度并计算 PHC 字符串。
///
/// # Errors
///
/// 长度不在 12–256 字符内或哈希失败时返回错误。
pub(crate) fn hash_password(password: &str) -> StoreResult<String> {
    let chars = password.chars().count();
    if !(PASSWORD_MIN..=PASSWORD_MAX).contains(&chars) {
        return Err(StoreError::Invalid(format!(
            "口令长度必须为 {PASSWORD_MIN}–{PASSWORD_MAX} 字符"
        )));
    }
    let salt = random::<16>()?;
    let hash = argon2()?
        .hash_password_with_salt(password.as_bytes(), &salt)
        .map_err(|error| StoreError::Invalid(error.to_string()))?;
    Ok(hash.to_string())
}

/// 核对口令。
///
/// # Errors
///
/// 库中哈希无法解析时返回错误。
fn verify_password(password: &str, stored: &str) -> StoreResult<bool> {
    let parsed =
        PasswordHash::new(stored).map_err(|error| StoreError::Corrupt(error.to_string()))?;
    Ok(argon2()?
        .verify_password(password.as_bytes(), &parsed)
        .is_ok())
}

/// 会话行。
#[derive(Debug)]
struct SessionRow {
    /// 用户。
    user_id: i64,
    /// CSRF 哈希。
    csrf_hash: String,
    /// 绝对过期。
    expires_at: Timestamp,
    /// 最近活动。
    last_seen_at: Timestamp,
}

/// 行 → 会话。
///
/// # Errors
///
/// 列内容不合法时返回错误。
fn session_row(row: &Row<'_>) -> StoreResult<SessionRow> {
    Ok(SessionRow {
        user_id: row.get(0)?,
        csrf_hash: row.get(1)?,
        expires_at: parse_ts(&row.get::<_, String>(2)?)?,
        last_seen_at: parse_ts(&row.get::<_, String>(3)?)?,
    })
}

/// API Token 行。
#[derive(Debug)]
struct TokenRow {
    /// Token ID。
    token_id: i64,
    /// 库存哈希。
    stored: String,
    /// 用户。
    user_id: i64,
    /// 作用域列表（逗号分隔）。
    scopes: String,
    /// 过期时间。
    expires_at: Timestamp,
}

/// 行 → API Token。
///
/// # Errors
///
/// 列内容不合法时返回错误。
fn token_row(row: &Row<'_>) -> StoreResult<TokenRow> {
    Ok(TokenRow {
        token_id: row.get(0)?,
        stored: row.get(1)?,
        user_id: row.get(2)?,
        scopes: row.get(3)?,
        expires_at: parse_ts(&row.get::<_, String>(4)?)?,
    })
}

/// 按 ID 读取用户。
///
/// # Errors
///
/// 用户不存在时返回错误。
fn user_by_id(conn: &Connection, user_id: i64) -> StoreResult<User> {
    query_opt(
        conn,
        "SELECT id, username, role, must_change_password, disabled FROM users WHERE id = ?1",
        [user_id],
        |row| {
            Ok(User {
                id: row.get(0)?,
                username: row.get(1)?,
                role: row.get::<_, String>(2)?.parse()?,
                must_change_password: row.get(3)?,
                disabled: row.get(4)?,
            })
        },
    )?
    .ok_or(StoreError::NotFound("用户"))
}

/// 某个计数主体的剩余退避秒数。
///
/// # Errors
///
/// 查询失败时返回错误。
fn throttle_remaining(
    conn: &Connection,
    scope: &str,
    subject: &str,
    now: Timestamp,
) -> StoreResult<u64> {
    let row = query_opt(
        conn,
        "SELECT failures, last_at FROM login_failures WHERE scope = ?1 AND subject = ?2",
        [scope, subject],
        |row| Ok((to_u64(row.get(0)?)?, parse_ts(&row.get::<_, String>(1)?)?)),
    )?;
    let Some((failures, last_at)) = row else {
        return Ok(0);
    };
    let until = last_at
        .unix_seconds()
        .saturating_add(backoff_delay(failures));
    Ok(until.saturating_sub(now.unix_seconds()))
}

/// 记一次失败。
///
/// # Errors
///
/// 写入失败时返回错误。
fn record_failure(
    tx: &Transaction<'_>,
    scope: &str,
    subject: &str,
    now: Timestamp,
) -> StoreResult<()> {
    exec(tx,
        "INSERT INTO login_failures (scope, subject, failures, last_at) VALUES (?1, ?2, 1, ?3)
         ON CONFLICT (scope, subject) DO UPDATE SET failures = failures + 1, last_at = excluded.last_at",
        [scope, subject, &ts(now)?],
    )?;
    Ok(())
}

/// 写入会话，返回原值。
///
/// # Errors
///
/// 随机源或写入失败时返回错误。
fn insert_session(
    tx: &Transaction<'_>,
    user_id: i64,
    attempt: &LoginAttempt,
    now: Timestamp,
) -> StoreResult<NewSession> {
    let session_id = random_token()?;
    let csrf_token = random_token()?;
    let expires_at = now.saturating_add(SESSION_ABSOLUTE);
    exec(tx,
        "INSERT INTO sessions (id_hash, user_id, csrf_hash, created_at, expires_at, last_seen_at, ip, user_agent)
         VALUES (?1, ?2, ?3, ?4, ?5, ?4, ?6, ?7)",
        rusqlite::params![
            sha256_hex(session_id.as_bytes()),
            user_id,
            sha256_hex(csrf_token.as_bytes()),
            ts(now)?,
            ts(expires_at)?,
            attempt.ip,
            attempt.user_agent
        ],
    )?;
    Ok(NewSession {
        session_id,
        csrf_token,
        expires_at,
    })
}

#[cfg(test)]
mod tests;
