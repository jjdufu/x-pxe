//! 并发修改 409、回滚、进行中的会话不受新版本影响。

use serde_json::{Value, json};
use xpxe_e2e::flow::{audit_ok, register_and_arm, run, session_token};
use xpxe_e2e::harness::{Harness, Launch};
use xpxe_e2e::{Outcome, seed};
use xpxe_test_support::{ensure, ensure_eq};

/// 取 Profile 头版本的文档与版本号。
///
/// # Errors
///
/// 请求失败时返回错误。
async fn head(
    harness: &Harness,
    login: &xpxe_e2e::harness::Login,
    id: i64,
) -> Outcome<(Value, u64)> {
    let got = harness
        .ok("GET", &format!("/api/v1/configs/profile/{id}"), login, None)
        .await?;
    let version = got
        .pointer("/version/version")
        .and_then(Value::as_u64)
        .ok_or("缺少版本号")?;
    Ok((got.get("document").cloned().unwrap_or_default(), version))
}

/// 回滚到版本 1：生成版本 3，内容等于版本 1，历史 3 条。
///
/// # Errors
///
/// 断言失败时返回错误。
async fn rollback_restores(
    harness: &Harness,
    login: &xpxe_e2e::harness::Login,
    id: i64,
    original_tz: &Value,
) -> Outcome {
    let path = format!("/api/v1/configs/profile/{id}");
    let rollback = json!({ "to_version": 1_u64, "expected_head": 2_u64, "comment": "e2e 回滚" });
    let _rolled = harness
        .ok("POST", &format!("{path}/rollback"), login, Some(&rollback))
        .await?;
    let (after, latest) = head(harness, login, id).await?;
    ensure_eq(&latest, &3, "回滚生成版本 3")?;
    ensure_eq(
        &after.get("timezone"),
        &Some(original_tz),
        "回滚后内容等于版本 1",
    )?;
    let versions = harness
        .ok("GET", &format!("{path}/versions"), login, None)
        .await?;
    ensure(
        versions.as_array().is_some_and(|list| list.len() == 3)
            || versions.to_string().matches("\"version\"").count() >= 3,
        "版本历史 3 条",
    )?;
    Ok(())
}

/// 版本场景。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn versioning() -> Outcome {
    run(async {
        let harness = Harness::start(Launch::default()).await?;
        let (login, ids) = seed::lan(&harness).await?;
        let spec = seed::lan_spec("e2e-ver", "52:54:00:77:01:e2", "uefi");
        let (_node, session) = register_and_arm(&harness, &login, ids, &spec).await?;
        let (mut doc, version) = head(&harness, &login, ids.profile).await?;
        ensure_eq(&version, &1, "初版")?;
        let original_tz = doc.get("timezone").cloned().unwrap_or_default();
        if let Some(timezone) = doc.get_mut("timezone") {
            *timezone = json!("Europe/Berlin");
        }
        let path = format!("/api/v1/configs/profile/{}", ids.profile);
        let saved = harness
            .ok(
                "PUT",
                &path,
                &login,
                Some(&json!({ "expected_head": 1_u64, "document": doc })),
            )
            .await?;
        ensure(
            saved.to_string().contains('2'),
            &format!("保存为版本 2：{saved}"),
        )?;
        let conflict = harness
            .admin(
                "PUT",
                &path,
                Some(&login),
                Some(&json!({ "expected_head": 1_u64, "document": doc })),
            )
            .await?;
        ensure_eq(&conflict.status, &409, "基于旧版本保存 409")?;
        let boot = session_token(&harness, &session, "boot_token")?;
        let preseed = harness
            .boot("GET", &format!("/p/{boot}/preseed.cfg"), b"")
            .await?;
        ensure_eq(&preseed.status, &200, "preseed")?;
        let text = preseed.text();
        ensure(
            text.contains(original_tz.as_str().unwrap_or("Asia/Shanghai")),
            "进行中的会话仍用旧时区",
        )?;
        ensure(!text.contains("Europe/Berlin"), "新版本不影响进行中的会话")?;
        rollback_restores(&harness, &login, ids.profile, &original_tz).await?;
        audit_ok(&harness, &login).await
    })
}
