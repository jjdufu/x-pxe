//! 认证、授权、CSRF、令牌隔离、登录退避与端口隔离。

use serde_json::json;
use xpxe_e2e::flow::{agent_event, audit_ok, register_and_arm, run, session_token};
use xpxe_e2e::harness::{Harness, Launch, Login};
use xpxe_e2e::http::Call;
use xpxe_e2e::{Outcome, seed};
use xpxe_test_support::{ensure, ensure_eq};

/// 管理端权限：无会话 401、缺 CSRF 403、viewer arm 403；引导端口不暴露管理 API。
///
/// # Errors
///
/// 断言失败时返回错误。
async fn admin_guards(harness: &Harness, login: &Login, node: i64) -> Outcome {
    let anonymous = harness.admin("GET", "/api/v1/nodes", None, None).await?;
    ensure_eq(&anonymous.status, &401, "无会话 401")?;
    let call = Call {
        method: "POST",
        path: &format!("/api/v1/nodes/{node}/disarm"),
        headers: vec![
            ("Cookie", login.cookie.clone()),
            ("Content-Type", "application/json".to_owned()),
        ],
        body: b"{}".to_vec(),
    };
    let no_csrf = harness.admin_raw(&call).await?;
    ensure_eq(&no_csrf.status, &403, "缺 CSRF 403")?;
    let body =
        json!({ "username": "watcher", "password": "Watcher-Passw0rd-2026!", "role": "viewer" });
    let _user = harness
        .ok("POST", "/api/v1/users", login, Some(&body))
        .await?;
    let viewer = harness.login("watcher", "Watcher-Passw0rd-2026!").await?;
    let denied = harness
        .admin(
            "POST",
            &format!("/api/v1/nodes/{node}/arm"),
            Some(&viewer),
            Some(&json!({})),
        )
        .await?;
    ensure_eq(&denied.status, &403, "viewer arm 403")?;
    ensure(
        denied.text().contains("FORBIDDEN"),
        &format!("按角色拒绝：{}", denied.text()),
    )?;
    let boot_port = harness.boot("GET", "/api/v1/nodes", b"").await?;
    ensure_eq(&boot_port.status, &404, "引导端口 /api/v1/nodes 404")?;
    Ok(())
}

/// 令牌跨种类与会话关闭后均被拒绝。
///
/// # Errors
///
/// 断言失败时返回错误。
async fn token_guards(harness: &Harness, login: &Login, node: i64, session: &str) -> Outcome {
    let boot = session_token(harness, session, "boot_token")?;
    let agent = session_token(harness, session, "agent_token")?;
    let preseed_by_agent = harness
        .boot("GET", &format!("/p/{agent}/preseed.cfg"), b"")
        .await?;
    ensure_eq(&preseed_by_agent.status, &403, "agent 令牌取 preseed 403")?;
    let agent_by_boot = agent_event(harness, &boot, session, "agent.started").await?;
    ensure_eq(&agent_by_boot.status, &403, "Boot 令牌报 agent 事件 403")?;
    let _disarmed = harness
        .ok(
            "POST",
            &format!("/api/v1/nodes/{node}/disarm"),
            login,
            Some(&json!({})),
        )
        .await?;
    harness.wait_state(login, node, "idle").await?;
    let closed = harness
        .boot("GET", &format!("/p/{boot}/boot.ipxe"), b"")
        .await?;
    ensure_eq(&closed.status, &403, "会话关闭后脚本 403")?;
    let unknown = harness
        .boot("GET", "/p/not-a-real-token-0000000000/boot.ipxe", b"")
        .await?;
    ensure_eq(&unknown.status, &403, "未知令牌 403")?;
    Ok(())
}

/// 连续错误口令后登录被退避（429 + `Retry-After`）。
///
/// # Errors
///
/// 断言失败时返回错误。
async fn login_backoff(harness: &Harness) -> Outcome {
    let wrong = json!({ "username": "watcher", "password": "definitely-wrong-password" });
    let mut throttled = None;
    for _attempt in 0_u32..8 {
        let reply = harness
            .admin("POST", "/api/v1/auth/login", None, Some(&wrong))
            .await?;
        if reply.status == 429 {
            throttled = Some(reply);
            break;
        }
        ensure_eq(&reply.status, &401, "错误口令 401")?;
    }
    let reply = throttled.ok_or("8 次错误口令后仍未退避")?;
    ensure(reply.header("retry-after").is_some(), "带 Retry-After")?;
    Ok(())
}

/// 安全场景。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn security() -> Outcome {
    run(async {
        let harness = Harness::start(Launch::default()).await?;
        let (login, ids) = seed::lan(&harness).await?;
        let spec = seed::lan_spec("e2e-sec", "52:54:00:77:01:e2", "uefi");
        let (node, session) = register_and_arm(&harness, &login, ids, &spec).await?;
        admin_guards(&harness, &login, node).await?;
        token_guards(&harness, &login, node, &session).await?;
        login_backoff(&harness).await?;
        audit_ok(&harness, &login).await
    })
}
