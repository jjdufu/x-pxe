//! 共享流程：LAN 装机全链路（DHCP 回放 → TFTP → iPXE → preseed → 安装器与 agent 事件 → `Ready`）。

use serde_json::{Value, json};
use xpxe_test_support::{ensure, ensure_eq};

use crate::Outcome;
use crate::bootnet::Sim;
use crate::harness::{Harness, Login};
use crate::seed::{self, Ids, asset_fixture};

/// 在多线程运行时上执行异步用例。
///
/// # Errors
///
/// 运行时创建失败或用例失败时返回错误。
pub fn run<F: Future<Output = Outcome>>(case: F) -> Outcome {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?
        .block_on(case)
}

/// 一个 LAN 用例。
#[derive(Debug, Clone, Copy)]
pub struct LanCase<'case> {
    /// 主机名。
    pub hostname: &'case str,
    /// MAC（冒号分隔）。
    pub mac: &'case str,
    /// 固件 `uefi` 或 `bios`。
    pub firmware: &'case str,
    /// 固件阶段 DISCOVER 夹具。
    pub pxe_fixture: &'case str,
    /// iPXE 阶段 DISCOVER 夹具（回放时 `chaddr` 改写为本机 MAC）。
    pub ipxe_fixture: &'case str,
    /// 期望的启动文件名后缀。
    pub file_suffix: &'case str,
    /// 期望 TFTP 下载到的 xpxe-assets 夹具。
    pub firmware_fixture: &'case str,
}

/// 管理端审计链校验通过。
///
/// # Errors
///
/// 链断开或请求失败时返回错误。
pub async fn audit_ok(harness: &Harness, login: &Login) -> Outcome {
    let report = harness
        .ok("GET", "/api/v1/audit/verify", login, None)
        .await?;
    ensure(
        report.get("first_broken").is_some_and(Value::is_null),
        &format!("审计链完整：{report}"),
    )?;
    ensure(
        report
            .get("checked")
            .and_then(Value::as_u64)
            .unwrap_or_default()
            > 0,
        "审计非空",
    )?;
    Ok(())
}

/// 会话目录中的令牌原文。
///
/// # Errors
///
/// 文件缺失时返回错误。
pub fn session_token(harness: &Harness, session: &str, name: &str) -> Outcome<String> {
    Ok(std::fs::read_to_string(
        harness
            .data_dir()
            .join("cache/sessions")
            .join(session)
            .join(name),
    )?
    .trim()
    .to_owned())
}

/// 登记并 arm，返回（节点 ID，会话 ID）。
///
/// # Errors
///
/// 请求失败时返回错误。
pub async fn register_and_arm(
    harness: &Harness,
    login: &Login,
    ids: Ids,
    spec: &Value,
) -> Outcome<(i64, String)> {
    let node = seed::register(harness, login, spec, ids).await?;
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
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    ensure(!session.is_empty(), "arm 返回会话 ID")?;
    harness.wait_state(login, node, "armed").await?;
    Ok((node, session))
}

/// 安装器阶段：取脚本与 preseed（一次性），上报 `installer.started/finished`。
///
/// # Errors
///
/// 任一断言失败时返回错误。
pub async fn installer(harness: &Harness, boot_token: &str, session: &str) -> Outcome {
    let script = harness
        .boot("GET", &format!("/p/{boot_token}/boot.ipxe"), b"")
        .await?;
    ensure_eq(&script.status, &200, "iPXE 脚本")?;
    ensure(script.text().starts_with("#!ipxe"), "脚本以 #!ipxe 开头")?;
    let preseed = harness
        .boot("GET", &format!("/p/{boot_token}/preseed.cfg"), b"")
        .await?;
    ensure_eq(&preseed.status, &200, "preseed 首次")?;
    let again = harness
        .boot("GET", &format!("/p/{boot_token}/preseed.cfg"), b"")
        .await?;
    ensure_eq(&again.status, &410, "preseed 第二次 410")?;
    for kind in ["installer.started", "installer.finished"] {
        let body = format!("event={kind} event_id={session}.installer");
        let reply = harness
            .boot(
                "POST",
                &format!("/p/{boot_token}/events/{kind}"),
                body.as_bytes(),
            )
            .await?;
        ensure(
            reply.status < 300,
            &format!("{kind} → {}：{}", reply.status, reply.text()),
        )?;
    }
    Ok(())
}

/// agent 阶段：计划只认 agent 令牌，上报 `agent.started/finished`。
///
/// # Errors
///
/// 任一断言失败时返回错误。
pub async fn agent(
    harness: &Harness,
    boot_token: &str,
    agent_token: &str,
    session: &str,
) -> Outcome {
    let denied = harness
        .boot("GET", &format!("/p/{boot_token}/plan.json"), b"")
        .await?;
    ensure_eq(&denied.status, &403, "Boot 令牌取计划被拒")?;
    let plan = harness
        .boot("GET", &format!("/p/{agent_token}/plan.json"), b"")
        .await?;
    ensure_eq(&plan.status, &200, "agent 令牌取计划")?;
    let _plan_json = plan.json()?;
    for kind in ["agent.started", "agent.finished"] {
        let body = json!({ "event_id": format!("{session}.{kind}"), "kind": kind, "payload": {} })
            .to_string();
        let reply = harness
            .boot("POST", &format!("/p/{agent_token}/events"), body.as_bytes())
            .await?;
        ensure(
            reply.status < 300,
            &format!("{kind} → {}：{}", reply.status, reply.text()),
        )?;
    }
    Ok(())
}

/// 完整 LAN 装机。
///
/// # Errors
///
/// 任一断言失败时返回错误。
pub async fn lan_install(harness: &Harness, case: LanCase<'_>) -> Outcome {
    let (login, ids) = seed::lan(harness).await?;
    let spec = seed::lan_spec(case.hostname, case.mac, case.firmware);
    let (node, session) = register_and_arm(harness, &login, ids, &spec).await?;
    let mut sim = Sim::new(harness.running()?.internal_addr(), &harness.token_file())?;
    sim.sync().await?;
    let now = harness.app()?.now();
    let first = sim.replay(case.pxe_fixture, now)?.ok_or("固件阶段无应答")?;
    ensure(
        first.file.starts_with("fw/") && first.file.ends_with(case.file_suffix),
        &format!("启动文件 {}", first.file),
    )?;
    let bytes = sim.tftp(&first.file).await?;
    ensure(
        *bytes == *asset_fixture(case.firmware_fixture)?,
        "TFTP 字节与上传一致",
    )?;
    let mac = xpxe_domain::MacAddr::parse(case.mac)?;
    let second = sim
        .replay_as(case.ipxe_fixture, Some(mac), now)?
        .ok_or("iPXE 阶段无应答")?;
    let boot_token = session_token(harness, &session, "boot_token")?;
    ensure(
        second.file.ends_with(&format!("/p/{boot_token}/boot.ipxe")),
        &format!("会话 URL {}", second.file),
    )?;
    let agent_token = session_token(harness, &session, "agent_token")?;
    installer(harness, &boot_token, &session).await?;
    agent(harness, &boot_token, &agent_token, &session).await?;
    harness.wait_state(&login, node, "ready").await?;
    sim.sync().await?;
    ensure(
        sim.replay(case.pxe_fixture, now)?.is_none(),
        "Ready 后 DISCOVER 无应答",
    )?;
    let mac_path = case.mac.replace(':', "-");
    let exit = harness
        .boot("GET", &format!("/boot/mac/{mac_path}"), b"")
        .await?;
    ensure(
        exit.text().contains("exit"),
        &format!("/boot/mac 返回 exit：{}", exit.text()),
    )?;
    audit_ok(harness, &login).await
}
