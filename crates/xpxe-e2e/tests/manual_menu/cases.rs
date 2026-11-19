//! 菜单关闭时 `/m/*` 404；分块上传与识别；开启后命中 MAC 得到菜单；`sanboot`/`kernel_initrd` 渲染；
//! Range 读取；嵌入 ISO 的 initrd；Armed 节点仍走状态机通道；选择回调与审计；删除 ISO 后条目不可用。

use serde_json::{Value, json};
use xpxe_e2e::flow::{audit_ok, register_and_arm, run};
use xpxe_e2e::harness::{Harness, Launch, Login};
use xpxe_e2e::http::{Call, Reply, send};
use xpxe_e2e::isofix::{FixtureFile, build};
use xpxe_e2e::{Outcome, seed};
use xpxe_test_support::{ensure, ensure_eq};

/// 菜单白名单中的 MAC。
const MAC: &str = "52:54:00:77:14:01";
/// 不在白名单中的 MAC。
const OTHER: &str = "52:54:00:77:14:02";
/// 在白名单中、同时被 arm 的节点 MAC。
const ARMED: &str = "52:54:00:77:14:03";

/// 引导端请求（可带 Range）。
///
/// # Errors
///
/// 连接失败时返回错误。
async fn boot_get(harness: &Harness, path: &str, range: Option<&str>) -> Outcome<Reply> {
    let headers = range.map_or_else(Vec::new, |value| vec![("Range", value.to_owned())]);
    send(
        harness.running()?.boot_addr(),
        None,
        &Call {
            method: "GET",
            path,
            headers,
            body: Vec::new(),
        },
    )
    .await
}

/// 分块上传一个 ISO，返回完成响应；中途故意用错偏移验证 409。
///
/// # Errors
///
/// 请求失败或断言失败时返回错误。
async fn upload(
    harness: &Harness,
    login: &Login,
    iso: &[u8],
    expected: Option<&str>,
) -> Outcome<Reply> {
    let begun = harness
        .ok(
            "POST",
            "/api/v1/isos/uploads",
            login,
            Some(
                &json!({ "label": "PVE fixture", "size": iso.len(), "expected_sha256": expected }),
            ),
        )
        .await?;
    let id = begun
        .get("id")
        .and_then(Value::as_str)
        .ok_or("缺少上传 ID")?
        .to_owned();
    let half = iso.len().checked_div(2).ok_or("长度")?;
    let (first, second) = iso.split_at(half);
    let put = |offset: usize, body: &[u8]| {
        let path = format!("/api/v1/isos/uploads/{id}?offset={offset}");
        let headers = vec![
            ("Content-Type", "application/octet-stream".to_owned()),
            ("Cookie", login.cookie.clone()),
            ("X-CSRF-Token", login.csrf.clone()),
        ];
        let payload = body.to_vec();
        async move {
            harness
                .admin_raw(&Call {
                    method: "PUT",
                    path: &path,
                    headers,
                    body: payload,
                })
                .await
        }
    };
    ensure_eq(&put(0, first).await?.status, &200, "第一块")?;
    ensure_eq(&put(0, second).await?.status, &409, "偏移不符 409")?;
    let progress = harness
        .ok("GET", &format!("/api/v1/isos/uploads/{id}"), login, None)
        .await?;
    ensure_eq(
        &progress.get("received").and_then(Value::as_u64),
        &Some(u64::try_from(half)?),
        "续传前查询已收字节",
    )?;
    ensure_eq(&put(half, second).await?.status, &200, "第二块")?;
    harness
        .admin(
            "POST",
            &format!("/api/v1/isos/uploads/{id}/complete"),
            Some(login),
            Some(&json!({})),
        )
        .await
}

/// 夹具 ISO：PVE 特征文件 + 一个说明文件。
///
/// # Errors
///
/// 构造失败时返回错误。
fn fixture() -> Outcome<Vec<u8>> {
    build(
        "PVE",
        &[
            FixtureFile {
                path: "/boot/linux26",
                data: b"KERNEL-BYTES",
            },
            FixtureFile {
                path: "/boot/initrd.img",
                data: b"INITRD",
            },
            FixtureFile {
                path: "/README.txt",
                data: b"fixture\n",
            },
        ],
    )
}

/// 菜单文档。
fn menu_doc(sha: &str, enabled: bool) -> Value {
    json!({
        "enabled": enabled,
        "allow_macs": [MAC, ARMED],
        "default_entry": "pve",
        "timeout_s": 5,
        "entries": [
            { "id": "pve", "title": "Proxmox VE", "kind": { "type": "kernel_initrd", "iso": sha,
              "kernel_path": "/boot/linux26", "initrd_path": "/boot/initrd.img", "cmdline": "ro quiet", "embed_iso": true } },
            { "id": "rescue", "title": "Rescue", "kind": { "type": "sanboot", "iso": sha } },
            { "id": "local", "title": "Local", "kind": { "type": "exit" } }
        ]
    })
}

/// 等待 `pve` 条目的抽取产物就绪，返回（内核，initrd）摘要。
///
/// # Errors
///
/// 超时时返回错误。
async fn wait_extracts(harness: &Harness, login: &Login) -> Outcome<(String, String)> {
    for _attempt in 0..100_u32 {
        let status = harness
            .ok("GET", "/api/v1/boot-menu/status", login, None)
            .await?;
        let action = status
            .pointer("/entries/0/action")
            .cloned()
            .unwrap_or_default();
        if action.get("type").and_then(Value::as_str) == Some("kernel") {
            let field = |name: &str| {
                action
                    .get(name)
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .ok_or("缺少摘要")
            };
            return Ok((field("kernel")?, field("initrd")?));
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    Err("抽取产物未就绪".into())
}

/// 菜单关闭：`/boot/mac` 返回 exit，`/m/*` 404。
///
/// # Errors
///
/// 断言失败时返回错误。
async fn closed(harness: &Harness, sha: &str) -> Outcome {
    let by_mac = boot_get(harness, "/boot/mac/52-54-00-77-14-01", None).await?;
    ensure(by_mac.text().contains("exit"), "菜单关闭时 exit")?;
    for path in [
        "/m/menu.ipxe?mac=52-54-00-77-14-01".to_owned(),
        format!("/m/iso/{sha}.iso"),
        "/m/select?mac=52-54-00-77-14-01&entry=m-pve".to_owned(),
    ] {
        ensure_eq(&boot_get(harness, &path, None).await?.status, &404, &path)?;
    }
    Ok(())
}

/// 菜单开启：命中白名单链到菜单，未命中仍 exit，菜单渲染两类条目。
///
/// # Errors
///
/// 断言失败时返回错误。
async fn routed(harness: &Harness, sha: &str, kernel: &str) -> Outcome {
    let hit = boot_get(harness, "/boot/mac/52-54-00-77-14-01", None)
        .await?
        .text();
    ensure(
        hit.contains("/m/menu.ipxe?mac=52-54-00-77-14-01"),
        &format!("命中白名单链到菜单：{hit}"),
    )?;
    let miss = boot_get(
        harness,
        &format!("/boot/mac/{}", OTHER.replace(':', "-")),
        None,
    )
    .await?
    .text();
    ensure(
        miss.contains("exit") && !miss.contains("/m/"),
        "未命中仍 exit",
    )?;
    let menu = boot_get(harness, "/m/menu.ipxe?mac=52-54-00-77-14-01", None)
        .await?
        .text();
    ensure(
        menu.contains(&format!(
            "kernel ${{base}}/m/x/{kernel} initrd=initrd ro quiet"
        )),
        &menu,
    )?;
    ensure(
        menu.contains(&format!("sanboot --no-describe ${{base}}/m/iso/{sha}.iso")),
        "sanboot 条目",
    )?;
    ensure(menu.contains("--default m-pve"), "默认项")?;
    let selected = boot_get(harness, "/m/select?mac=52-54-00-77-14-01&entry=m-pve", None).await?;
    ensure_eq(&selected.status, &200, "选择回调")?;
    Ok(())
}

/// Range 读取 ISO；内核原样；initrd 末尾追加含整份 ISO 的 newc 段；未知产物 404。
///
/// # Errors
///
/// 断言失败时返回错误。
async fn content(harness: &Harness, sha: &str, iso: &[u8], extracts: &(String, String)) -> Outcome {
    let (kernel, initrd) = extracts;
    let ranged = boot_get(
        harness,
        &format!("/m/iso/{sha}.iso"),
        Some("bytes=32768-32773"),
    )
    .await?;
    ensure_eq(&ranged.status, &206, "Range 206")?;
    ensure_eq(
        &ranged.body.as_slice(),
        &iso.get(32768..32774).ok_or("夹具过短")?,
        "Range 内容",
    )?;
    let total = format!("/{}", iso.len());
    ensure(
        ranged
            .header("content-range")
            .is_some_and(|value| value.ends_with(&total)),
        "Content-Range",
    )?;
    let kernel_body = boot_get(harness, &format!("/m/x/{kernel}"), None).await?;
    ensure_eq(
        &kernel_body.body.as_slice(),
        &&b"KERNEL-BYTES"[..],
        "内核原样",
    )?;
    let initrd_body = boot_get(harness, &format!("/m/x/{initrd}"), None)
        .await?
        .body;
    ensure(
        initrd_body.starts_with(b"INITRD")
            && initrd_body
                .windows(12)
                .any(|window| window == b"proxmox.iso\0"),
        "newc 段含 /proxmox.iso",
    )?;
    let head = iso.get(..4096).ok_or("夹具过短")?;
    ensure(
        initrd_body.windows(head.len()).any(|window| window == head),
        "initrd 中含整份 ISO",
    )?;
    let unknown = format!("/m/x/{}", "0".repeat(64));
    ensure_eq(
        &boot_get(harness, &unknown, None).await?.status,
        &404,
        "未知产物 404",
    )?;
    Ok(())
}

/// 上传夹具：先用错误的期望摘要被拒，再正常上传并核对识别结果，返回 SHA-256。
///
/// # Errors
///
/// 断言失败时返回错误。
async fn upload_fixture(harness: &Harness, login: &Login, iso: &[u8]) -> Outcome<String> {
    let wrong = "0".repeat(64);
    let rejected = upload(harness, login, iso, Some(&wrong)).await?;
    ensure_eq(&rejected.status, &422, "期望摘要不符拒收")?;
    let done = upload(harness, login, iso, None).await?;
    ensure_eq(&done.status, &200, &done.text())?;
    let body = done.json()?;
    ensure_eq(
        &body.pointer("/detection/family").and_then(Value::as_str),
        &Some("pve"),
        "识别为 PVE 系",
    )?;
    ensure_eq(
        &body
            .pointer("/detection/kernel_path")
            .and_then(Value::as_str),
        &Some("/boot/linux26"),
        "预填内核路径",
    )?;
    ensure_eq(
        &body
            .pointer("/detection/embed_iso")
            .and_then(Value::as_bool),
        &Some(true),
        "预填 embed_iso",
    )?;
    let tree = harness
        .ok(
            "GET",
            &format!(
                "/api/v1/isos/{}/tree",
                body.get("sha256")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
            ),
            login,
            None,
        )
        .await?;
    ensure(tree.to_string().contains("/boot/initrd.img"), "目录树")?;
    Ok(body
        .get("sha256")
        .and_then(Value::as_str)
        .ok_or("缺少摘要")?
        .to_owned())
}

/// 删除 ISO 后条目不可用、ISO 404；随后关闭菜单，`/m/*` 404。
///
/// # Errors
///
/// 断言失败时返回错误。
async fn delete_and_close(harness: &Harness, login: &Login, sha: &str, menu_id: i64) -> Outcome {
    let deleted = harness
        .ok("DELETE", &format!("/api/v1/isos/{sha}"), login, None)
        .await?;
    ensure(deleted.to_string().contains("pve"), "删除结果列出引用条目")?;
    let menu = boot_get(harness, "/m/menu.ipxe?mac=52-54-00-77-14-01", None)
        .await?
        .text();
    ensure(
        menu.contains("entry unavailable: iso deleted"),
        "删除后条目不可用",
    )?;
    ensure_eq(
        &boot_get(harness, &format!("/m/iso/{sha}.iso"), None)
            .await?
            .status,
        &404,
        "删除后 ISO 404",
    )?;
    let head = harness
        .ok(
            "GET",
            &format!("/api/v1/configs/boot_menu/{menu_id}"),
            login,
            None,
        )
        .await?;
    let version = head
        .pointer("/version/version")
        .and_then(Value::as_u64)
        .ok_or("版本")?;
    let off = json!({ "enabled": false, "allow_macs": [MAC], "entries": [] });
    let _saved = harness
        .ok(
            "PUT",
            &format!("/api/v1/configs/boot_menu/{menu_id}"),
            login,
            Some(&json!({ "expected_head": version, "document": off })),
        )
        .await?;
    ensure_eq(
        &boot_get(harness, "/m/menu.ipxe?mac=52-54-00-77-14-01", None)
            .await?
            .status,
        &404,
        "关闭后 404",
    )?;
    Ok(())
}

/// 审计链包含上传、菜单版本变更、命中与选择记录，且校验通过。
///
/// # Errors
///
/// 断言失败时返回错误。
async fn audited(harness: &Harness, login: &Login) -> Outcome {
    let audit = harness
        .ok("GET", "/api/v1/audit?limit=500", login, None)
        .await?
        .to_string();
    for action in [
        "iso.upload",
        "config.create",
        "config.save",
        "manual.menu_hit",
        "manual.select",
        "iso.delete",
    ] {
        ensure(audit.contains(action), &format!("审计含 {action}"))?;
    }
    audit_ok(harness, login).await
}

/// 手动引导菜单端到端。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn manual_menu() -> Outcome {
    run(async {
        let harness = Harness::start(Launch::default()).await?;
        let (login, ids) = seed::lan(&harness).await?;
        let iso = fixture()?;
        let sha = upload_fixture(&harness, &login, &iso).await?;
        closed(&harness, &sha).await?;
        let created = harness
            .ok(
                "POST",
                "/api/v1/configs/boot_menu",
                &login,
                Some(&json!({ "name": "default", "document": menu_doc(&sha, true) })),
            )
            .await?;
        let menu_id = created
            .get("object_id")
            .or_else(|| created.get("id"))
            .and_then(Value::as_i64)
            .ok_or_else(|| format!("缺少对象 ID：{created}"))?;
        let extracts = wait_extracts(&harness, &login).await?;
        routed(&harness, &sha, &extracts.0).await?;
        content(&harness, &sha, &iso, &extracts).await?;
        let spec = seed::lan_spec("e2e-manual", ARMED, "uefi");
        let (_node, _session) = register_and_arm(&harness, &login, ids, &spec).await?;
        let armed = boot_get(
            &harness,
            &format!("/boot/mac/{}", ARMED.replace(':', "-")),
            None,
        )
        .await?
        .text();
        ensure(
            armed.contains("/p/") && !armed.contains("/m/"),
            "Armed 节点始终走状态机通道",
        )?;
        delete_and_close(&harness, &login, &sha, menu_id).await?;
        audited(&harness, &login).await
    })
}
