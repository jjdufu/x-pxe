//! 每个带超时的状态拨动时钟后进入期望状态；迟到的回调被拒为 `StaleSession`。

use serde_json::json;
use xpxe_e2e::flow::{agent_event, audit_ok, installer_event, run, session_token};
use xpxe_e2e::harness::{ADMIN_PASSWORD, Harness, Launch, Login};
use xpxe_e2e::{Outcome, seed};
use xpxe_test_support::{ensure, ensure_eq};

/// 一次会话的令牌。
struct Armed {
    /// 会话 ID。
    session: String,
    /// 引导令牌。
    boot: String,
    /// agent 令牌。
    agent: String,
}

/// arm 并读取令牌。
///
/// # Errors
///
/// 请求失败时返回错误。
async fn arm(harness: &Harness, login: &Login, node: i64) -> Outcome<Armed> {
    let armed = harness
        .ok(
            "POST",
            &format!("/api/v1/nodes/{node}/arm"),
            login,
            Some(&json!({})),
        )
        .await?;
    let session = armed
        .get("session_id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_owned();
    harness.wait_state(login, node, "armed").await?;
    Ok(Armed {
        boot: session_token(harness, &session, "boot_token")?,
        agent: session_token(harness, &session, "agent_token")?,
        session,
    })
}

/// 期望 2xx。
///
/// # Errors
///
/// 非 2xx 时返回错误。
fn accepted(reply: &xpxe_e2e::http::Reply, what: &str) -> Outcome {
    ensure(
        reply.status < 300,
        &format!("{what} → {}：{}", reply.status, reply.text()),
    )?;
    Ok(())
}

/// 期望 409 `STALE_SESSION`。
///
/// # Errors
///
/// 不符时返回错误。
fn stale(reply: &xpxe_e2e::http::Reply, what: &str) -> Outcome {
    ensure_eq(&reply.status, &409, what)?;
    ensure(
        reply.text().contains("STALE_SESSION"),
        &format!("{what}：{}", reply.text()),
    )?;
    Ok(())
}

/// 拨动时钟后重新登录（管理会话也会过期）并等待状态。
///
/// # Errors
///
/// 超时时返回错误。
async fn expire(harness: &Harness, node: i64, secs: u64, want: &str) -> Outcome<Login> {
    harness.advance(secs).await?;
    let fresh = harness.login("admin", ADMIN_PASSWORD).await?;
    harness.wait_state(&fresh, node, want).await?;
    Ok(fresh)
}

/// 失败后复位并补一次心跳（下一次 arm 的前置检查需要）。
///
/// # Errors
///
/// 请求失败时返回错误。
async fn reset(harness: &Harness, login: &Login, node: i64) -> Outcome {
    let _reset = harness
        .ok(
            "POST",
            &format!("/api/v1/nodes/{node}/reset"),
            login,
            Some(&json!({})),
        )
        .await?;
    harness.wait_state(login, node, "idle").await?;
    seed::heartbeat(harness).await
}

/// `Armed` 超时回到 `Idle`；`Installing` 超时进入 `Failed`。
///
/// # Errors
///
/// 断言失败时返回错误。
async fn armed_and_installing(harness: &Harness, login: &mut Login, node: i64) -> Outcome {
    let first = arm(harness, login, node).await?;
    *login = expire(harness, node, 7_201, "idle").await?;
    stale(
        &installer_event(harness, &first.boot, &first.session, "installer.started").await?,
        "Armed 超时后的回调",
    )?;
    seed::heartbeat(harness).await?;
    let second = arm(harness, login, node).await?;
    accepted(
        &installer_event(harness, &second.boot, &second.session, "installer.started").await?,
        "started",
    )?;
    *login = expire(harness, node, 5_401, "failed").await?;
    stale(
        &installer_event(harness, &second.boot, &second.session, "installer.finished").await?,
        "Installing 超时后的回调",
    )?;
    reset(harness, login, node).await
}

/// `Installed` 与 `Configuring` 超时进入 `Failed`。
///
/// # Errors
///
/// 断言失败时返回错误。
async fn installed_and_configuring(harness: &Harness, login: &mut Login, node: i64) -> Outcome {
    let third = arm(harness, login, node).await?;
    accepted(
        &installer_event(harness, &third.boot, &third.session, "installer.started").await?,
        "started",
    )?;
    accepted(
        &installer_event(harness, &third.boot, &third.session, "installer.finished").await?,
        "finished",
    )?;
    *login = expire(harness, node, 1_801, "failed").await?;
    reset(harness, login, node).await?;
    let fourth = arm(harness, login, node).await?;
    accepted(
        &installer_event(harness, &fourth.boot, &fourth.session, "installer.started").await?,
        "started",
    )?;
    accepted(
        &installer_event(harness, &fourth.boot, &fourth.session, "installer.finished").await?,
        "finished",
    )?;
    accepted(
        &agent_event(harness, &fourth.agent, &fourth.session, "agent.started").await?,
        "agent.started",
    )?;
    *login = expire(harness, node, 3_601, "failed").await?;
    stale(
        &agent_event(harness, &fourth.agent, &fourth.session, "agent.finished").await?,
        "Configuring 超时后的回调",
    )
}

/// `Armed`、`Installing`、`Installed`、`Configuring` 四个超时。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn every_deadline_fires() -> Outcome {
    run(async {
        let harness = Harness::start(Launch::default()).await?;
        let (mut login, ids) = seed::lan(&harness).await?;
        let spec = seed::lan_spec("e2e-timer", "52:54:00:77:01:e2", "uefi");
        let node = seed::register(&harness, &login, &spec, ids).await?;
        armed_and_installing(&harness, &mut login, node).await?;
        installed_and_configuring(&harness, &mut login, node).await?;
        audit_ok(&harness, &login).await
    })
}
