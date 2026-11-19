//! 预览/差异、节点抽屉、浏览类列表、资产校验与引导网络操作端点。

use serde_json::{Value, json};
use xpxe_e2e::flow::{agent, audit_ok, installer, register_and_arm, run, session_token};
use xpxe_e2e::harness::{Harness, Launch, Login};
use xpxe_e2e::seed::Ids;
use xpxe_e2e::{Outcome, seed};
use xpxe_test_support::{ensure, ensure_eq};

/// 读取配置头版本文档。
///
/// # Errors
///
/// 请求失败时返回错误。
async fn document(harness: &Harness, login: &Login, kind: &str, id: i64) -> Outcome<Value> {
    let got = harness
        .ok("GET", &format!("/api/v1/configs/{kind}/{id}"), login, None)
        .await?;
    Ok(got.get("document").cloned().unwrap_or_default())
}

/// 预设与模块目录。
///
/// # Errors
///
/// 断言失败时返回错误。
async fn catalogs(harness: &Harness, login: &Login) -> Outcome {
    let presets = harness.ok("GET", "/api/v1/presets", login, None).await?;
    ensure(
        presets.pointer("/profile/nexus-lvm/ntp").is_some(),
        "预设含 nexus-lvm",
    )?;
    ensure(
        presets.pointer("/init_plan/deb13-init-scripts").is_some(),
        "预设含 deb13-init-scripts",
    )?;
    let modules = harness
        .ok("GET", "/api/v1/init-modules", login, None)
        .await?;
    let docs = modules
        .get("package_groups")
        .and_then(Value::as_array)
        .and_then(|groups| {
            groups
                .iter()
                .find(|group| group.get("id") == Some(&json!("docs_fonts")))
        })
        .and_then(|group| group.get("count"))
        .cloned();
    ensure_eq(&docs, &Some(json!(6_u64)), "docs_fonts 6 个包")?;
    ensure(
        modules
            .get("modules")
            .and_then(Value::as_array)
            .is_some_and(|list| list.len() > 10),
        "模块目录",
    )?;
    Ok(())
}

/// Profile 草稿预览：改 NTP 后 preseed 与字段差异随之变化；保存后版本差异含该字段。
///
/// # Errors
///
/// 断言失败时返回错误。
async fn profile_preview(harness: &Harness, login: &Login, ids: Ids) -> Outcome {
    let mut doc = document(harness, login, "profile", ids.profile).await?;
    if let Some(ntp) = doc.get_mut("ntp") {
        *ntp = json!(["10.9.8.7"]);
    }
    let body = json!({ "document": doc, "base": { "object_id": ids.profile, "version": 1_u64 } });
    let preview = harness
        .ok(
            "POST",
            "/api/v1/configs/profile/preview",
            login,
            Some(&body),
        )
        .await?;
    ensure_eq(&preview.get("ok"), &Some(&json!(true)), "预览成功")?;
    ensure(
        preview
            .to_string()
            .contains("clock-setup/ntp-server string 10.9.8.7"),
        "preseed 含新 NTP",
    )?;
    ensure(
        preview.pointer("/changes/0/path").is_some(),
        &format!(
            "字段差异：{}",
            preview.get("changes").cloned().unwrap_or_default()
        ),
    )?;
    let path = format!("/api/v1/configs/profile/{}", ids.profile);
    let _saved = harness
        .ok(
            "PUT",
            &path,
            login,
            Some(&json!({ "expected_head": 1_u64, "document": doc })),
        )
        .await?;
    let diff = harness
        .ok("GET", &format!("{path}/diff?from=1&to=2"), login, None)
        .await?;
    ensure(
        diff.to_string().contains("ntp"),
        &format!("版本差异含 ntp：{diff}"),
    )?;
    let invalid = harness
        .ok(
            "POST",
            "/api/v1/configs/profile/validate",
            login,
            Some(&json!({ "document": { "name": "x" } })),
        )
        .await?;
    ensure_eq(&invalid.get("ok"), &Some(&json!(false)), "非法草稿校验失败")?;
    Ok(())
}

/// 初始化计划预览：关闭 `docs_fonts` 后包数量减少 6。
///
/// # Errors
///
/// 断言失败时返回错误。
async fn plan_preview(harness: &Harness, login: &Login, ids: Ids) -> Outcome {
    let mut doc = document(harness, login, "init_plan", ids.plan).await?;
    let count = |reply: &Value| reply.get("package_count").and_then(Value::as_u64);
    let before = harness
        .ok(
            "POST",
            "/api/v1/configs/init_plan/preview",
            login,
            Some(&json!({ "document": doc })),
        )
        .await?;
    ensure(
        before
            .get("modules")
            .and_then(Value::as_array)
            .is_some_and(|list| !list.is_empty()),
        "逐模块脚本",
    )?;
    if let Some(modules) = doc.get_mut("modules").and_then(Value::as_object_mut) {
        let _old = modules.insert(
            "packages".to_owned(),
            json!({ "settings": { "groups": { "docs_fonts": false } } }),
        );
    }
    let after = harness
        .ok(
            "POST",
            "/api/v1/configs/init_plan/preview",
            login,
            Some(&json!({ "document": doc })),
        )
        .await?;
    ensure_eq(
        &count(&after),
        &count(&before).map(|total| total.saturating_sub(6)),
        "包数量 −6",
    )?;
    Ok(())
}

/// 装到 Ready 后的节点抽屉端点与总览。
///
/// # Errors
///
/// 断言失败时返回错误。
async fn node_drawer(harness: &Harness, login: &Login, ids: Ids) -> Outcome<i64> {
    let spec = seed::lan_spec("e2e-ui", "52:54:00:77:12:e2", "uefi");
    let (node, session) = register_and_arm(harness, login, ids, &spec).await?;
    let boot = session_token(harness, &session, "boot_token")?;
    let agent_token = session_token(harness, &session, "agent_token")?;
    installer(harness, &boot, &session).await?;
    agent(harness, &boot, &agent_token, &session).await?;
    harness.wait_state(login, node, "ready").await?;
    let timeline = harness
        .ok(
            "GET",
            &format!("/api/v1/nodes/{node}/timeline"),
            login,
            None,
        )
        .await?;
    ensure(
        timeline.as_array().is_some_and(|list| list.len() >= 4),
        "时间轴含安装事件",
    )?;
    let detail = harness
        .ok(
            "GET",
            &format!("/api/v1/nodes/{node}/sessions/{session}"),
            login,
            None,
        )
        .await?;
    ensure(detail.pointer("/pinned").is_some(), "会话固定版本")?;
    ensure(
        detail
            .get("events")
            .and_then(Value::as_array)
            .is_some_and(|list| !list.is_empty()),
        "会话事件",
    )?;
    let overview = harness.ok("GET", "/api/v1/overview", login, None).await?;
    ensure(
        overview.pointer("/states/by_state").is_some() && overview.get("outcomes").is_some(),
        "总览",
    )?;
    Ok(node)
}

/// 节点规格新版本：主机名不可改，其他字段可改。
///
/// # Errors
///
/// 断言失败时返回错误。
async fn node_update(harness: &Harness, login: &Login, ids: Ids, node: i64) -> Outcome {
    let current = harness
        .ok("GET", &format!("/api/v1/nodes/{node}/spec"), login, None)
        .await?;
    let head = current
        .pointer("/version/version")
        .and_then(Value::as_u64)
        .unwrap_or(1);
    let mut spec = seed::lan_spec("e2e-ui", "52:54:00:77:12:e2", "uefi");
    let body = |draft: &Value| json!({ "expected_head": head, "spec": draft, "profile_id": ids.profile, "init_plan_id": ids.plan });
    let path = format!("/api/v1/nodes/{node}");
    let _saved = harness.ok("PUT", &path, login, Some(&body(&spec))).await?;
    if let Some(hostname) = spec.get_mut("hostname") {
        *hostname = json!("e2e-renamed");
    }
    let refused = harness
        .admin("PUT", &path, Some(login), Some(&body(&spec)))
        .await?;
    ensure(
        refused.status == 422 || refused.status == 409,
        &format!("改主机名被拒：{}", refused.status),
    )?;
    Ok(())
}

/// 浏览类列表：用户、会话、令牌、历史、审计与导出。
///
/// # Errors
///
/// 断言失败时返回错误。
async fn listings(harness: &Harness, login: &Login) -> Outcome {
    let users = harness.ok("GET", "/api/v1/users", login, None).await?;
    ensure(users.to_string().contains("\"admin\""), "用户列表含 admin")?;
    let sessions = harness
        .ok("GET", "/api/v1/me/sessions", login, None)
        .await?;
    ensure(
        sessions.to_string().contains("\"current\":true"),
        "当前会话标记",
    )?;
    let _tokens = harness.ok("GET", "/api/v1/me/tokens", login, None).await?;
    let history = harness.ok("GET", "/api/v1/history", login, None).await?;
    ensure(
        history.as_array().is_some_and(|list| list.len() >= 4),
        "配置历史",
    )?;
    let audit = harness
        .ok(
            "GET",
            "/api/v1/audit?action=node.armed&page=1&page_size=10",
            login,
            None,
        )
        .await?;
    ensure(
        audit
            .get("total")
            .and_then(Value::as_u64)
            .is_some_and(|total| total >= 1),
        &format!("审计筛选：{audit}"),
    )?;
    let export = harness
        .admin("GET", "/api/v1/audit/export", Some(login), None)
        .await?;
    ensure_eq(&export.status, &200, "审计导出")?;
    ensure(export.text().lines().count() >= 3, "导出为 NDJSON")?;
    Ok(())
}

/// 资产校验/删除与引导网络操作。
///
/// # Errors
///
/// 断言失败时返回错误。
async fn operations(harness: &Harness, login: &Login) -> Outcome {
    let assets = harness.ok("GET", "/api/v1/assets", login, None).await?;
    let id = assets
        .pointer("/assets/0/id")
        .and_then(Value::as_i64)
        .ok_or("缺少资产")?;
    let verified = harness
        .ok(
            "POST",
            &format!("/api/v1/assets/{id}/verify"),
            login,
            Some(&json!({})),
        )
        .await?;
    ensure_eq(&verified.get("intact"), &Some(&json!(true)), "资产完整")?;
    let refused = harness
        .admin("DELETE", &format!("/api/v1/assets/{id}"), Some(login), None)
        .await?;
    ensure_eq(&refused.status, &409, "激活中的资产不可删除")?;
    let scan = harness
        .ok("POST", "/api/v1/bootnet/scan", login, Some(&json!({})))
        .await?;
    ensure_eq(&scan.get("probe_nonce"), &Some(&json!(1_u64)), "探测序号")?;
    let _ack = harness
        .ok(
            "POST",
            "/api/v1/bootnet/acknowledge",
            login,
            Some(&json!({ "servers": ["10.0.0.1"] })),
        )
        .await?;
    let snippets = harness
        .ok("GET", "/api/v1/bootnet/snippets", login, None)
        .await?;
    ensure(
        snippets
            .get("snippets")
            .and_then(Value::as_array)
            .is_some_and(|list| !list.is_empty()),
        "配置片段",
    )?;
    Ok(())
}

/// `WebUI` 端点场景。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn webui_api() -> Outcome {
    run(async {
        let harness = Harness::start(Launch::default()).await?;
        let (login, ids) = seed::lan(&harness).await?;
        catalogs(&harness, &login).await?;
        profile_preview(&harness, &login, ids).await?;
        plan_preview(&harness, &login, ids).await?;
        let node = node_drawer(&harness, &login, ids).await?;
        node_update(&harness, &login, ids, node).await?;
        listings(&harness, &login).await?;
        operations(&harness, &login).await?;
        audit_ok(&harness, &login).await
    })
}
