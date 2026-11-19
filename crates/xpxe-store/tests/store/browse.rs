//! 管理界面浏览查询：用户、会话、令牌、审计分页、事件详情与结果统计。

use serde_json::json;
use xpxe_domain::{EventId, NodeState};
use xpxe_store::{
    AuditQuery, AuditRecord, EventRecord, LoginAttempt, Role, Scope, TokenGrant, UserPatch,
};
use xpxe_test_support::{TempDataDir, TestResult, ensure, ensure_eq};

use crate::install::armed;
use crate::support::{at, open};

/// 口令。
const PASSWORD: &str = "correct-horse-battery";

/// 用户列表、修改角色、禁用即注销会话；会话与令牌列表不含原值。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn accounts_listing() -> TestResult {
    let dir = TempDataDir::new()?;
    let store = open(dir.path())?;
    let accounts = store.accounts();
    let user = accounts.create_user("ops", PASSWORD, Role::Viewer, at(0))?;
    let attempt = LoginAttempt {
        username: "ops".to_owned(),
        password: PASSWORD.to_owned(),
        ip: "10.0.0.9".to_owned(),
        user_agent: "ua".to_owned(),
    };
    let (_user, session) = accounts.login(&attempt, at(1))?;
    let listed = accounts.sessions(user.id)?;
    ensure_eq(&listed.len(), &1, "一个会话")?;
    let key = listed
        .first()
        .map_or_else(String::new, |item| item.key.clone());
    ensure_eq(&key.len(), &16, "标识长度")?;
    ensure(!session.session_id.contains(&key), "标识不是 Cookie 原值")?;
    let grant = TokenGrant {
        name: "ci".to_owned(),
        scopes: vec![Scope::Read],
        expires_at: at(86_400),
    };
    let (token_id, _raw) = accounts.issue_api_token(user.id, &grant, at(2))?;
    let tokens = accounts.tokens(user.id)?;
    ensure_eq(
        &tokens.first().map(|item| item.id),
        &Some(token_id),
        "令牌列表",
    )?;
    let patched = accounts.patch_user(
        user.id,
        UserPatch {
            role: Some(Role::Operator),
            disabled: None,
        },
    )?;
    ensure_eq(&patched.role, &Role::Operator, "改角色")?;
    ensure_eq(&accounts.users()?.len(), &1, "用户列表")?;
    ensure(accounts.revoke_session(user.id, "zz").is_err(), "非法标识")?;
    let _disabled = accounts.patch_user(
        user.id,
        UserPatch {
            role: None,
            disabled: Some(true),
        },
    )?;
    ensure(accounts.sessions(user.id)?.is_empty(), "禁用后会话清空")?;
    ensure(
        accounts.patch_user(999, UserPatch::default()).is_err(),
        "不存在",
    )?;
    Ok(())
}

/// 审计分页与筛选；事件详情与按日结果统计。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn audit_and_events() -> TestResult {
    let dir = TempDataDir::new()?;
    let store = open(dir.path())?;
    for n in 0..7_u64 {
        let record = AuditRecord {
            actor: if n % 2 == 0 { "admin" } else { "ops" }.to_owned(),
            ip: String::new(),
            action: "node.arm".to_owned(),
            target: format!("node:{n}"),
            outcome: "ok".to_owned(),
            detail: json!({}),
        };
        let _id = store.audit().append(&record, at(n))?;
    }
    let page = store.audit().page(&AuditQuery {
        actor: Some("admin".to_owned()),
        limit: 2,
        ..AuditQuery::default()
    })?;
    ensure_eq(&page.total, &4, "筛选总数")?;
    ensure_eq(&page.entries.len(), &2, "分页")?;
    ensure_eq(
        &page.actors,
        &vec!["admin".to_owned(), "ops".to_owned()],
        "候选",
    )?;
    let (node_id, issued) = armed(&store)?;
    let event = EventRecord {
        event_id: EventId::parse("admin.arm.1")?,
        session_id: Some(issued.session.clone()),
        at: at(3),
        source: "admin".to_owned(),
        kind: "arm".to_owned(),
        from_state: NodeState::Idle,
        to_state: NodeState::Armed,
        deadline_at: None,
        active_session_id: Some(issued.session.clone()),
        payload: json!({ "user": "admin" }),
    };
    let _inserted = store.nodes().record(node_id, &event)?;
    let details = store.nodes().details(None, 10)?;
    ensure_eq(&details.len(), &1, "跨节点")?;
    ensure_eq(
        &details.first().map(|item| item.hostname.clone()),
        &Some("node01".to_owned()),
        "主机名",
    )?;
    ensure_eq(
        &store.nodes().session_events(issued.session.as_str())?.len(),
        &1,
        "会话事件",
    )?;
    ensure(store.nodes().outcomes(at(0))?.is_empty(), "尚无结果")?;
    Ok(())
}
