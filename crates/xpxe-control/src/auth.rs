//! 管理端认证与授权：会话 Cookie + CSRF、个人 API Token、角色。

use std::net::SocketAddr;
use std::sync::Arc;

use axum::Json;
use axum::extract::{FromRequestParts, Path, State};
use axum::response::{IntoResponse as _, Response};
use http::request::Parts;
use http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use serde::Deserialize;
use serde_json::{Value, json};
use xpxe_domain::Timestamp;
use xpxe_store::{AuditRecord, LoginAttempt, Role, Scope, TokenGrant, User};

use crate::app::App;
use crate::{Failure, Outcome, internal};

/// 会话 Cookie 名。
pub(crate) const COOKIE: &str = "xpxe_session";
/// CSRF 请求头。
pub(crate) const CSRF_HEADER: &str = "x-csrf-token";

/// 连接对端地址（由监听循环注入请求扩展）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Peer(pub(crate) SocketAddr);

/// 审计中的操作者。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Actor {
    /// 名称（用户名、`system`、`installer`…）。
    pub(crate) name: String,
    /// 来源 IP。
    pub(crate) ip: String,
}

impl Actor {
    /// 系统内部操作者。
    pub(crate) fn system() -> Self {
        Self {
            name: "system".to_owned(),
            ip: String::new(),
        }
    }

    /// 生成审计记录。
    pub(crate) fn record(
        &self,
        action: &str,
        target: &str,
        outcome: &str,
        detail: Value,
    ) -> AuditRecord {
        AuditRecord {
            actor: self.name.clone(),
            ip: self.ip.clone(),
            action: action.to_owned(),
            target: target.to_owned(),
            outcome: outcome.to_owned(),
            detail,
        }
    }
}

/// 已认证的调用方。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Principal {
    /// 用户。
    pub(crate) user: User,
    /// 生效角色（API Token 取用户角色与作用域的较小者）。
    pub(crate) role: Role,
    /// 会话 ID（API Token 调用为 `None`）。
    pub(crate) session: Option<String>,
    /// 来源 IP。
    pub(crate) ip: String,
}

/// 角色等级。
const fn rank(role: Role) -> u8 {
    match role {
        Role::Viewer => 1,
        Role::Operator => 2,
        Role::Admin => 3,
    }
}

/// 作用域对应的角色。
const fn scope_role(scope: Scope) -> Role {
    match scope {
        Scope::Read => Role::Viewer,
        Scope::Operate => Role::Operator,
        Scope::Admin => Role::Admin,
    }
}

impl Principal {
    /// 要求至少某角色。
    ///
    /// # Errors
    ///
    /// 角色不足时返回 403。
    pub(crate) fn require(&self, min: Role) -> Outcome<()> {
        if rank(self.role) >= rank(min) {
            Ok(())
        } else {
            Err(Failure::forbidden(
                "FORBIDDEN",
                format!("需要 {} 角色", min.as_str()),
            ))
        }
    }

    /// 审计操作者。
    pub(crate) fn actor(&self) -> Actor {
        Actor {
            name: self.user.username.clone(),
            ip: self.ip.clone(),
        }
    }
}

/// 请求来源 IP（提取器）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ClientIp(pub(crate) String);

impl<S: Send + Sync> FromRequestParts<S> for ClientIp {
    type Rejection = Failure;

    fn from_request_parts(
        parts: &mut Parts,
        _state: &S,
    ) -> impl Future<Output = Result<Self, Self::Rejection>> + Send {
        std::future::ready(Ok(Self(peer_ip(parts))))
    }
}

/// 请求来源 IP。
pub(crate) fn peer_ip(parts: &Parts) -> String {
    parts
        .extensions
        .get::<Peer>()
        .map_or_else(String::new, |peer| peer.0.ip().to_string())
}

/// 从 `Cookie` 头取会话 ID。
fn cookie_session(headers: &HeaderMap) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|line| line.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(name, _)| *name == COOKIE)
        .map(|(_, value)| value.to_owned())
}

/// `Authorization: Bearer xpxe_…`。
fn bearer(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
        .map(str::to_owned)
}

/// 改密前允许访问的路径。
const PASSWORD_CHANGE_PATHS: [&str; 3] =
    ["/api/v1/me", "/api/v1/me/password", "/api/v1/auth/logout"];

/// 用 API Token 认证。
///
/// # Errors
///
/// 令牌无效时返回 401。
fn by_token(app: &App, raw: &str, ip: String, now: Timestamp) -> Outcome<Principal> {
    let found = app
        .store
        .accounts()
        .api_token(raw, now)?
        .ok_or_else(unauthenticated)?;
    let best = found
        .scopes
        .iter()
        .map(|scope| scope_role(*scope))
        .max_by_key(|role| rank(*role));
    let role = best.map_or(Role::Viewer, |scope| {
        if rank(scope) < rank(found.user.role) {
            scope
        } else {
            found.user.role
        }
    });
    Ok(Principal {
        user: found.user,
        role,
        session: None,
        ip,
    })
}

/// 用会话 Cookie 认证；写操作检查 CSRF。
///
/// # Errors
///
/// 会话无效时返回 401，CSRF 不符时返回 403。
fn by_session(app: &App, parts: &Parts, id: String, now: Timestamp) -> Outcome<Principal> {
    let found = app
        .store
        .accounts()
        .session(&id, now)?
        .ok_or_else(unauthenticated)?;
    let safe = matches!(parts.method, Method::GET | Method::HEAD | Method::OPTIONS);
    if !safe {
        let presented = parts
            .headers
            .get(CSRF_HEADER)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default();
        if presented.is_empty() || !found.csrf_matches(presented) {
            return Err(Failure::forbidden(
                "CSRF_FAILED",
                "缺少或错误的 X-CSRF-Token",
            ));
        }
    }
    Ok(Principal {
        role: found.user.role,
        user: found.user,
        session: Some(id),
        ip: peer_ip(parts),
    })
}

/// 401。
fn unauthenticated() -> Failure {
    Failure::new(StatusCode::UNAUTHORIZED, "UNAUTHENTICATED", "需要登录")
}

impl FromRequestParts<Arc<App>> for Principal {
    type Rejection = Failure;

    fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<App>,
    ) -> impl Future<Output = Result<Self, Self::Rejection>> + Send {
        std::future::ready(authenticate(parts, state))
    }
}

/// 认证一个请求：Bearer API 令牌或会话 Cookie（非 GET 须带 CSRF）。
///
/// # Errors
///
/// 未登录、账户禁用或须先改密时返回 401/403。
fn authenticate(parts: &Parts, app: &App) -> Outcome<Principal> {
    let now = app.now();
    let principal = if let Some(raw) = bearer(&parts.headers) {
        by_token(app, &raw, peer_ip(parts), now)?
    } else if let Some(id) = cookie_session(&parts.headers) {
        by_session(app, parts, id, now)?
    } else {
        return Err(unauthenticated());
    };
    if principal.user.disabled {
        return Err(unauthenticated());
    }
    if principal.user.must_change_password && !PASSWORD_CHANGE_PATHS.contains(&parts.uri.path()) {
        return Err(Failure::forbidden(
            "PASSWORD_CHANGE_REQUIRED",
            "首次登录必须修改密码",
        ));
    }
    Ok(principal)
}

/// 登录请求。
#[derive(Debug, Deserialize)]
pub(crate) struct LoginBody {
    /// 用户名。
    username: String,
    /// 密码。
    password: String,
}

/// 用户 JSON。
fn user_json(user: &User) -> Value {
    json!({
        "id": user.id, "username": user.username, "role": user.role,
        "must_change_password": user.must_change_password,
    })
}

/// 会话 Cookie。
///
/// # Errors
///
/// 失败时返回错误。
fn session_cookie(app: &App, value: &str, clear: bool) -> Outcome<HeaderValue> {
    let secure = if app.settings.tls.is_some() {
        "; Secure"
    } else {
        ""
    };
    let expire = if clear { "; Max-Age=0" } else { "" };
    HeaderValue::from_str(&format!(
        "{COOKIE}={value}; HttpOnly; SameSite=Strict; Path=/{secure}{expire}"
    ))
    .map_err(internal)
}

/// `POST /auth/login`。
///
/// # Errors
///
/// 凭据错误（401）、被退避（429）或存储失败时返回错误。
pub(crate) async fn login(
    State(app): State<Arc<App>>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    Json(body): Json<LoginBody>,
) -> Outcome<Response> {
    let attempt = LoginAttempt {
        username: body.username.clone(),
        password: body.password,
        ip: ip.clone(),
        user_agent: headers
            .get(header::USER_AGENT)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_owned(),
    };
    let actor = Actor {
        name: body.username,
        ip,
    };
    let store_app = Arc::clone(&app);
    let now = app.now();
    let result =
        tokio::task::spawn_blocking(move || store_app.store.accounts().login(&attempt, now))
            .await
            .map_err(internal)?;
    match result {
        Ok((user, session)) => {
            app.audit(&actor.record("auth.login", &format!("user:{}", user.id), "ok", json!({})));
            let cookie = session_cookie(&app, &session.session_id, false)?;
            let reply = json!({ "user": user_json(&user), "csrf_token": session.csrf_token });
            Ok(([(header::SET_COOKIE, cookie)], Json(reply)).into_response())
        }
        Err(error) => {
            app.audit(&actor.record(
                "auth.login",
                "user",
                "denied",
                json!({ "reason": error.to_string() }),
            ));
            let failure = Failure::from(error.clone());
            if let xpxe_store::StoreError::Throttled { retry_after } = error {
                let value = HeaderValue::from(retry_after);
                return Ok(([(header::RETRY_AFTER, value)], failure).into_response());
            }
            Err(failure)
        }
    }
}

/// `POST /auth/logout`。
///
/// # Errors
///
/// 存储失败时返回错误。
pub(crate) async fn logout(State(app): State<Arc<App>>, principal: Principal) -> Outcome<Response> {
    if let Some(id) = &principal.session {
        let _removed = app.store.accounts().logout(id)?;
    }
    app.audit(&principal.actor().record(
        "auth.logout",
        &format!("user:{}", principal.user.id),
        "ok",
        json!({}),
    ));
    let cookie = session_cookie(&app, "", true)?;
    Ok(([(header::SET_COOKIE, cookie)], StatusCode::NO_CONTENT).into_response())
}

/// `GET /me`。
pub(crate) async fn me(principal: Principal) -> Json<Value> {
    Json(json!({ "user": user_json(&principal.user), "role": principal.role }))
}

/// 改密请求。
#[derive(Debug, Deserialize)]
pub(crate) struct PasswordBody {
    /// 旧密码。
    old_password: String,
    /// 新密码。
    new_password: String,
}

/// `POST /me/password`：保留当前会话，注销其他会话。
///
/// # Errors
///
/// 旧密码错误（401）、新密码不合规（400）或存储失败时返回错误。
pub(crate) async fn change_password(
    State(app): State<Arc<App>>,
    principal: Principal,
    Json(body): Json<PasswordBody>,
) -> Outcome<StatusCode> {
    let store_app = Arc::clone(&app);
    let user_id = principal.user.id;
    let keep = principal.session.clone();
    tokio::task::spawn_blocking(move || {
        store_app.store.accounts().change_password(
            user_id,
            &body.old_password,
            &body.new_password,
            keep.as_deref(),
        )
    })
    .await
    .map_err(internal)??;
    app.audit(&principal.actor().record(
        "auth.password",
        &format!("user:{user_id}"),
        "ok",
        json!({}),
    ));
    Ok(StatusCode::NO_CONTENT)
}

/// 新建 API Token 请求。
#[derive(Debug, Deserialize)]
pub(crate) struct TokenBody {
    /// 名称。
    name: String,
    /// 作用域：`read`、`operate`、`admin`。
    scopes: Vec<String>,
    /// 有效天数（默认 90）。
    ttl_days: Option<u64>,
}

/// `POST /me/tokens`：明文令牌只返回这一次。
///
/// # Errors
///
/// 作用域未知或存储失败时返回错误。
pub(crate) async fn issue_token(
    State(app): State<Arc<App>>,
    principal: Principal,
    Json(body): Json<TokenBody>,
) -> Outcome<Json<Value>> {
    let scopes = body
        .scopes
        .iter()
        .map(|raw| raw.parse::<Scope>())
        .collect::<Result<Vec<_>, _>>()?;
    let days = body.ttl_days.unwrap_or(90);
    let now = app.now();
    let grant = TokenGrant {
        name: body.name,
        scopes,
        expires_at: now.saturating_add(std::time::Duration::from_secs(days.saturating_mul(86_400))),
    };
    let (id, token) = app
        .store
        .accounts()
        .issue_api_token(principal.user.id, &grant, now)?;
    app.audit(&principal.actor().record(
        "token.issue",
        &format!("token:{id}"),
        "ok",
        json!({ "name": grant.name }),
    ));
    Ok(Json(
        json!({ "id": id, "token": token, "expires_at": grant.expires_at }),
    ))
}

/// `DELETE /me/tokens/{id}`。
///
/// # Errors
///
/// 令牌不存在（404）或存储失败时返回错误。
pub(crate) async fn revoke_token(
    State(app): State<Arc<App>>,
    principal: Principal,
    Path(id): Path<i64>,
) -> Outcome<StatusCode> {
    if !app
        .store
        .accounts()
        .revoke_api_token(principal.user.id, id)?
    {
        return Err(Failure::not_found("令牌不存在"));
    }
    app.audit(
        &principal
            .actor()
            .record("token.revoke", &format!("token:{id}"), "ok", json!({})),
    );
    Ok(StatusCode::NO_CONTENT)
}

/// 新建用户请求。
#[derive(Debug, Deserialize)]
pub(crate) struct UserBody {
    /// 用户名。
    username: String,
    /// 初始密码。
    password: String,
    /// 角色。
    role: String,
}

/// `POST /users`（仅 admin）。
///
/// # Errors
///
/// 角色不足（403）、参数不合法（400）或存储失败时返回错误。
pub(crate) async fn create_user(
    State(app): State<Arc<App>>,
    principal: Principal,
    Json(body): Json<UserBody>,
) -> Outcome<Json<Value>> {
    principal.require(Role::Admin)?;
    let role: Role = body.role.parse()?;
    let store_app = Arc::clone(&app);
    let now = app.now();
    let user = tokio::task::spawn_blocking(move || {
        store_app
            .store
            .accounts()
            .create_user(&body.username, &body.password, role, now)
    })
    .await
    .map_err(internal)??;
    app.audit(&principal.actor().record(
        "user.create",
        &format!("user:{}", user.id),
        "ok",
        json!({ "role": role }),
    ));
    Ok(Json(user_json(&user)))
}
