//! 探测不带凭据 → 信任 → 部署七步（1 前置检查、2–6 会话部署、7 `installer.started` 后自动 `EjectMedia`）；
//! 指纹不符时部署失败；BIOS 固件的 Redfish 节点被拒绝。

use std::time::Duration;

use serde_json::{Value, json};
use xpxe_e2e::flow::{audit_ok, installer_event, register_and_arm, run, session_token};
use xpxe_e2e::harness::{Harness, Launch, Login};
use xpxe_e2e::{Outcome, seed};
use xpxe_test_support::{ensure, ensure_eq};

use crate::mock::{self, Shared};

/// Redfish 节点规格。
fn redfish_spec(hostname: &str, mac: &str, firmware: &str, bmc: &str) -> Value {
    json!({
        "hostname": hostname, "fqdn": null, "mac": mac, "os_family": "debian13",
        "firmware": firmware, "channel": { "type": "redfish", "bmc": bmc }, "network": { "type": "dhcp" },
    })
}

/// 启动模拟 BMC（`cert` 为 xpxe-redfish 夹具证书名）并登记，返回（状态，对象 ID）。
///
/// # Errors
///
/// 启动或登记失败时返回错误。
async fn bmc(harness: &Harness, login: &Login, name: &str, cert: &str) -> Outcome<(Shared, i64)> {
    let (state, handler) = mock::mock();
    let port = mock::start(cert, handler).await?;
    let body = json!({ "name": name, "endpoint": format!("https://127.0.0.1:{port}"), "username": "admin", "password": "secret" });
    let created = harness
        .ok("POST", "/api/v1/bmcs", login, Some(&body))
        .await?;
    let id = created
        .pointer("/version/object_id")
        .and_then(Value::as_i64)
        .ok_or("登记 BMC 未返回 ID")?;
    Ok((state, id))
}

/// 探测（不带凭据）并信任给定指纹（`None` 表示信任探测到的指纹），返回探测到的指纹。
///
/// # Errors
///
/// 请求失败时返回错误。
async fn probe_and_trust(
    harness: &Harness,
    login: &Login,
    id: i64,
    state: &Shared,
    pin: Option<&str>,
) -> Outcome<String> {
    let probed = harness
        .ok(
            "POST",
            &format!("/api/v1/bmcs/{id}/probe"),
            login,
            Some(&json!({})),
        )
        .await?;
    let fingerprint = probed
        .get("fingerprint")
        .and_then(Value::as_str)
        .ok_or("探测未返回指纹")?
        .to_owned();
    ensure_eq(&mock::credentialed(state), &0, "探测不带凭据")?;
    let trusted = pin.unwrap_or(&fingerprint).to_owned();
    let _pinned = harness
        .ok(
            "POST",
            &format!("/api/v1/bmcs/{id}/trust"),
            login,
            Some(&json!({ "fingerprint": trusted })),
        )
        .await?;
    Ok(fingerprint)
}

/// 等待 `EjectMedia` 次数超过 `before`（最多 3 秒）。
///
/// # Errors
///
/// 超时时返回错误。
async fn wait_eject(state: &Shared, before: usize) -> Outcome {
    for _attempt in 0_u32..30 {
        if mock::count(state, "POST", "VirtualMedia.EjectMedia") > before {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Err("installer.started 后未收到 EjectMedia".into())
}

/// 正常部署与自动弹出。
///
/// # Errors
///
/// 断言失败时返回错误。
async fn deploy_and_eject(harness: &Harness, login: &Login, ids: seed::Ids) -> Outcome<String> {
    let (state, id) = bmc(harness, login, "lab-bmc", "bmc").await?;
    let fingerprint = probe_and_trust(harness, login, id, &state, None).await?;
    let spec = redfish_spec("e2e-redfish", "52:54:00:77:01:a4", "uefi", "lab-bmc");
    let (node, session) = register_and_arm(harness, login, ids, &spec).await?;
    let deployed = harness
        .ok(
            "POST",
            &format!("/api/v1/nodes/{node}/redfish/deploy"),
            login,
            Some(&json!({})),
        )
        .await?;
    let steps = deployed
        .get("steps")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let names: Vec<&str> = steps
        .iter()
        .filter_map(|step| step.get("step").and_then(Value::as_str))
        .collect();
    ensure_eq(
        &names,
        &vec!["precheck", "insert", "inserted", "boot_override", "reset"],
        &format!("部署第 1–6 步：{deployed}"),
    )?;
    ensure(
        steps
            .iter()
            .all(|step| step.get("ok") == Some(&json!(true))),
        "每步成功",
    )?;
    ensure_eq(
        &mock::count(&state, "POST", "VirtualMedia.InsertMedia"),
        &1,
        "插入一次",
    )?;
    let before = mock::count(&state, "POST", "VirtualMedia.EjectMedia");
    let boot = session_token(harness, &session, "boot_token")?;
    let started = installer_event(harness, &boot, &session, "installer.started").await?;
    ensure(
        started.status < 300,
        &format!("installer.started → {}", started.status),
    )?;
    harness.wait_state(login, node, "installing").await?;
    wait_eject(&state, before).await?;
    Ok(fingerprint)
}

/// Redfish 场景。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn redfish() -> Outcome {
    run(async {
        let harness = Harness::start(Launch::default()).await?;
        let (login, ids) = seed::lan(&harness).await?;
        let fingerprint = deploy_and_eject(&harness, &login, ids).await?;

        let (other_state, other_id) = bmc(&harness, &login, "wrong-bmc", "other").await?;
        let _probed =
            probe_and_trust(&harness, &login, other_id, &other_state, Some(&fingerprint)).await?;
        let spec = redfish_spec("e2e-wrongpin", "52:54:00:77:01:a5", "uefi", "wrong-bmc");
        let (node, _session) = register_and_arm(&harness, &login, ids, &spec).await?;
        let failed = harness
            .admin(
                "POST",
                &format!("/api/v1/nodes/{node}/redfish/deploy"),
                Some(&login),
                Some(&json!({})),
            )
            .await?;
        ensure(
            failed.status >= 400,
            &format!("指纹不符时部署失败：{} {}", failed.status, failed.text()),
        )?;
        ensure_eq(
            &mock::count(&other_state, "POST", "VirtualMedia.InsertMedia"),
            &0,
            "未向错误 BMC 发出插入",
        )?;

        let bios = redfish_spec("e2e-redfish-bios", "52:54:00:77:01:a3", "bios", "lab-bmc");
        let body = json!({ "spec": bios, "profile_id": ids.profile, "init_plan_id": ids.plan });
        let rejected = harness
            .admin("POST", "/api/v1/nodes", Some(&login), Some(&body))
            .await?;
        ensure_eq(&rejected.status, &422, "BIOS + Redfish 被拒绝")?;
        ensure(
            rejected.text().contains("redfish_requires_uefi"),
            &rejected.text(),
        )?;
        audit_ok(&harness, &login).await
    })
}
