//! 账户、会话、API Token 与登录退避。

use std::fs;

use xpxe_store::{LoginAttempt, Role, Scope, StoreError, TokenGrant};
use xpxe_test_support::{TempDataDir, TestResult, ensure, ensure_eq};

use crate::support::{at, open};

/// 口令。
const PASSWORD: &str = "correct-horse-battery";

/// 登录参数。
fn attempt(username: &str, password: &str, ip: &str) -> LoginAttempt {
    LoginAttempt {
        username: username.to_owned(),
        password: password.to_owned(),
        ip: ip.to_owned(),
        user_agent: "test".to_owned(),
    }
}

/// 库为空时从文件创建 admin 且必须改密；之后不再创建。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn initial_admin_from_file() -> TestResult {
    let dir = TempDataDir::new()?;
    let store = open(&dir.path().join("data"))?;
    let accounts = store.accounts();
    ensure(
        accounts.ensure_initial_admin(None, at(0)).is_err(),
        "缺口令文件",
    )?;
    let file = dir.path().join("admin.pw");
    fs::write(&file, format!("{PASSWORD}\n"))?;
    ensure(accounts.ensure_initial_admin(Some(&file), at(0))?, "已创建")?;
    ensure(
        !accounts.ensure_initial_admin(Some(&file), at(1))?,
        "不重复创建",
    )?;
    let (user, _session) = accounts.login(&attempt("admin", PASSWORD, "10.0.0.1"), at(2))?;
    ensure(user.must_change_password, "必须改密")?;
    ensure_eq(&user.role, &Role::Admin, "角色")?;
    Ok(())
}

/// 用户不存在与口令错误返回同一错误；连续失败第 5 次起退避，退避期间正确口令也被拒。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn login_errors_and_backoff() -> TestResult {
    let dir = TempDataDir::new()?;
    let store = open(dir.path())?;
    let accounts = store.accounts();
    let _ops = accounts.create_user("ops", PASSWORD, Role::Operator, at(0))?;
    ensure_eq(
        &accounts
            .login(&attempt("ghost", PASSWORD, "10.0.0.9"), at(1))
            .map(|_ok| ()),
        &Err(StoreError::InvalidCredentials),
        "用户不存在",
    )?;
    for n in 1_u32..=5 {
        ensure_eq(
            &accounts
                .login(&attempt("ops", "wrong-password!", "10.0.0.2"), at(10))
                .map(|_ok| ()),
            &Err(StoreError::InvalidCredentials),
            &format!("第 {n} 次失败"),
        )?;
    }
    ensure_eq(
        &accounts
            .login(&attempt("ops", PASSWORD, "10.0.0.3"), at(11))
            .map(|_ok| ()),
        &Err(StoreError::Throttled { retry_after: 1 }),
        "按用户名退避（换 IP 也不行）",
    )?;
    ensure_eq(
        &accounts
            .login(&attempt("someone", PASSWORD, "10.0.0.2"), at(11))
            .map(|_ok| ()),
        &Err(StoreError::Throttled { retry_after: 1 }),
        "按 IP 退避（不暴露用户是否存在）",
    )?;
    ensure_eq(
        &accounts
            .login(&attempt("ops", "wrong-password!", "10.0.0.2"), at(12))
            .map(|_ok| ()),
        &Err(StoreError::InvalidCredentials),
        "第 6 次失败",
    )?;
    ensure_eq(
        &accounts
            .login(&attempt("ops", PASSWORD, "10.0.0.2"), at(15))
            .map(|_ok| ()),
        &Err(StoreError::Throttled { retry_after: 1 }),
        "第 6 次后退避 4 s",
    )?;
    let (user, _session) = accounts.login(&attempt("ops", PASSWORD, "10.0.0.2"), at(16))?;
    ensure_eq(&user.username.as_str(), &"ops", "退避结束可登录")?;
    let after_reset = accounts.login(&attempt("ops", "wrong-password!", "10.0.0.2"), at(17));
    ensure(after_reset.is_err(), "成功后计数清零")?;
    let relogin = accounts.login(&attempt("ops", PASSWORD, "10.0.0.2"), at(17));
    ensure(relogin.is_ok(), "清零后一次失败不退避")?;
    Ok(())
}

/// 会话空闲 30 min、绝对 12 h；CSRF 核对；注销。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn session_lifetimes() -> TestResult {
    let dir = TempDataDir::new()?;
    let store = open(dir.path())?;
    let accounts = store.accounts();
    let _viewer = accounts.create_user("viewer", PASSWORD, Role::Viewer, at(0))?;
    let (_idle_user, idle) = accounts.login(&attempt("viewer", PASSWORD, "10.0.0.1"), at(0))?;
    let live = accounts
        .session(&idle.session_id, at(29 * 60))?
        .ok_or("29 min 有效")?;
    ensure(live.csrf_matches(&idle.csrf_token), "CSRF 匹配")?;
    ensure(!live.csrf_matches("forged"), "伪造 CSRF")?;
    ensure(
        accounts.session(&idle.session_id, at(58 * 60))?.is_some(),
        "活动刷新空闲计时",
    )?;
    ensure(
        accounts.session(&idle.session_id, at(89 * 60))?.is_none(),
        "空闲 31 min 失效",
    )?;
    let (_busy_user, busy) = accounts.login(&attempt("viewer", PASSWORD, "10.0.0.1"), at(0))?;
    let mut minute = 20_u64;
    while minute < 12 * 60 {
        ensure(
            accounts
                .session(&busy.session_id, at(minute * 60))?
                .is_some(),
            &format!("第 {minute} 分钟有效"),
        )?;
        minute = minute.saturating_add(20);
    }
    ensure(
        accounts.session(&busy.session_id, at(12 * 3600))?.is_none(),
        "绝对 12 h 失效",
    )?;
    let (_gone_user, gone) = accounts.login(&attempt("viewer", PASSWORD, "10.0.0.1"), at(0))?;
    ensure(accounts.logout(&gone.session_id)?, "注销")?;
    ensure(
        accounts.session(&gone.session_id, at(1))?.is_none(),
        "注销后无效",
    )?;
    ensure(accounts.session("nonsense", at(1))?.is_none(), "未知会话")?;
    Ok(())
}

/// 改密注销该用户其他会话，保留当前会话；旧口令错误被拒。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn password_change_revokes_other_sessions() -> TestResult {
    let dir = TempDataDir::new()?;
    let store = open(dir.path())?;
    let accounts = store.accounts();
    let user = accounts.create_user("admin2", PASSWORD, Role::Admin, at(0))?;
    let (_u1, keep) = accounts.login(&attempt("admin2", PASSWORD, "10.0.0.1"), at(1))?;
    let (_u2, other) = accounts.login(&attempt("admin2", PASSWORD, "10.0.0.5"), at(2))?;
    ensure_eq(&accounts.session_count(user.id)?, &2, "两个会话")?;
    ensure_eq(
        &accounts.change_password(user.id, "not-the-password", "new-password-123", None),
        &Err(StoreError::InvalidCredentials),
        "旧口令错误",
    )?;
    accounts.change_password(
        user.id,
        PASSWORD,
        "new-password-123",
        Some(&keep.session_id),
    )?;
    ensure(
        accounts.session(&keep.session_id, at(3))?.is_some(),
        "当前会话保留",
    )?;
    ensure(
        accounts.session(&other.session_id, at(3))?.is_none(),
        "其他会话注销",
    )?;
    ensure(
        accounts
            .login(&attempt("admin2", "new-password-123", "10.0.0.1"), at(4))
            .is_ok(),
        "新口令可登录",
    )?;
    Ok(())
}

/// API Token：前缀、作用域、过期必须 ≤ 365 天、过期失效、吊销。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn api_tokens() -> TestResult {
    let dir = TempDataDir::new()?;
    let store = open(dir.path())?;
    let accounts = store.accounts();
    let user = accounts.create_user("bot", PASSWORD, Role::Operator, at(0))?;
    let too_long = TokenGrant {
        name: "ci".to_owned(),
        scopes: vec![Scope::Read],
        expires_at: at(366 * 86_400),
    };
    ensure(
        accounts.issue_api_token(user.id, &too_long, at(0)).is_err(),
        "超过 365 天",
    )?;
    let empty = TokenGrant {
        scopes: Vec::new(),
        expires_at: at(3600),
        ..too_long.clone()
    };
    ensure(
        accounts.issue_api_token(user.id, &empty, at(0)).is_err(),
        "空作用域",
    )?;
    let grant = TokenGrant {
        scopes: vec![Scope::Read, Scope::Operate],
        expires_at: at(365 * 86_400),
        ..too_long
    };
    let (token_id, raw) = accounts.issue_api_token(user.id, &grant, at(0))?;
    ensure(raw.starts_with("xpxe_"), "前缀")?;
    let principal = accounts.api_token(&raw, at(10))?.ok_or("有效")?;
    ensure_eq(
        &principal.scopes,
        &vec![Scope::Read, Scope::Operate],
        "作用域",
    )?;
    ensure_eq(&principal.user.id, &user.id, "用户")?;
    ensure(
        accounts.api_token(&raw, at(365 * 86_400))?.is_none(),
        "过期",
    )?;
    ensure(accounts.api_token("xpxe_forged", at(10))?.is_none(), "伪造")?;
    ensure(accounts.api_token("bearer", at(10))?.is_none(), "无前缀")?;
    ensure(accounts.revoke_api_token(user.id, token_id)?, "吊销")?;
    ensure(accounts.api_token(&raw, at(10))?.is_none(), "吊销后无效")?;
    Ok(())
}
