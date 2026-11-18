//! 场景前置数据：内置预设建配置、离线上传夹具资产并激活、bootnet 心跳、节点登记。

use crate::Outcome;
use serde_json::{Value, json};
use xpxe_assets::{Actor, AssetKind, Catalog, Operation, VerifiedAsset, store_bytes};
use xpxe_domain::init::TargetPlan;
use xpxe_domain::{BootNetwork, DhcpRules, Health, Heartbeat, NetIface, Profile};
use xpxe_store::{AssetSource, Role};
use xpxe_test_support::{TestFailure, fixture_bytes};

use crate::harness::{Harness, Login};

/// 夹具内核对应的 ABI（与夹具 `Packages.xz` 对齐）。
pub const KERNEL_ABI: &str = "6.12.107+deb13-amd64";
/// 夹具内核字节。
pub const LINUX: &[u8] = b"e2e-linux-kernel-bytes";
/// 夹具 initrd 字节。
pub const INITRD: &[u8] = b"e2e-initrd-bytes";

/// 已建配置的对象 ID。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ids {
    /// Profile。
    pub profile: i64,
    /// 初始化计划。
    pub plan: i64,
}

/// xpxe-assets 夹具字节。
///
/// # Errors
///
/// 文件缺失时返回错误。
pub fn asset_fixture(path: &str) -> Outcome<Vec<u8>> {
    Ok(fixture_bytes(
        concat!(env!("CARGO_MANIFEST_DIR"), "/../xpxe-assets"),
        path,
    )?)
}

/// 新建配置对象，返回 ID。
///
/// # Errors
///
/// 请求失败时返回错误。
pub async fn create(
    harness: &Harness,
    login: &Login,
    kind: &str,
    name: &str,
    document: &Value,
) -> Outcome<i64> {
    let created = harness
        .ok(
            "POST",
            &format!("/api/v1/configs/{kind}"),
            login,
            Some(&json!({ "name": name, "document": document })),
        )
        .await?;
    created
        .get("id")
        .or_else(|| created.get("object_id"))
        .and_then(Value::as_i64)
        .ok_or_else(|| TestFailure::new(format!("创建 {kind} 未返回 id：{created}")).into())
}

/// 用 `nexus-lvm` 与 `deb13-init-scripts` 预设建配置，镜像指向模拟镜像；写入引导网络与 DHCP 规则。
///
/// # Errors
///
/// 预设或请求失败时返回错误。
pub async fn configs(harness: &Harness, login: &Login, rules: &DhcpRules) -> Outcome<Ids> {
    let mut profile = serde_json::to_value(Profile::load_preset("nexus-lvm")?)?;
    if let Some(mirror) = profile.get_mut("mirror") {
        *mirror = json!({ "scheme": "http", "host_port": harness.mirror().to_string(), "directory": "/debian" });
    }
    let plan = serde_json::to_value(TargetPlan::load_preset("deb13-init-scripts")?)?;
    let ids = Ids {
        profile: create(harness, login, "profile", "nexus-lvm", &profile).await?,
        plan: create(harness, login, "init_plan", "deb13-init-scripts", &plan).await?,
    };
    let network = BootNetwork::ProxyDhcp {
        iface: NetIface::parse("br-lab")?,
    };
    let _network = create(
        harness,
        login,
        "boot_network",
        "default",
        &serde_json::to_value(network)?,
    )
    .await?;
    let _rules = create(
        harness,
        login,
        "dhcp_rules",
        "default",
        &serde_json::to_value(rules)?,
    )
    .await?;
    Ok(ids)
}

/// 上传一个 iPXE 夹具，返回资产 ID。
///
/// # Errors
///
/// 上传失败时返回错误。
async fn upload(harness: &Harness, login: &Login, kind: AssetKind, fixture: &str) -> Outcome<i64> {
    let call = crate::http::Call {
        method: "POST",
        path: &format!("/api/v1/assets/upload?kind={}", kind.as_str()),
        headers: vec![
            ("Cookie", login.cookie.clone()),
            ("X-CSRF-Token", login.csrf.clone()),
            ("Content-Type", "application/octet-stream".to_owned()),
        ],
        body: asset_fixture(fixture)?,
    };
    let reply = harness.admin_raw(&call).await?;
    if reply.status != 200 {
        return Err(format!("上传 {fixture} → {}：{}", reply.status, reply.text()).into());
    }
    reply
        .json()?
        .get("id")
        .and_then(Value::as_i64)
        .ok_or_else(|| TestFailure::new("上传未返回 id").into())
}

/// 激活资产（跳过金丝雀，写明理由）。
///
/// # Errors
///
/// 请求失败时返回错误。
pub async fn activate(
    harness: &Harness,
    login: &Login,
    kind: AssetKind,
    asset_id: i64,
) -> Outcome<()> {
    let body =
        json!({ "kind": kind.as_str(), "asset_id": asset_id, "skip_canary_reason": "e2e 夹具" });
    let _version = harness
        .ok("POST", "/api/v1/assets/activate", login, Some(&body))
        .await?;
    Ok(())
}

/// 登记 netboot 夹具（内核与 initrd 按内容寻址落盘），返回资产 ID。
///
/// # Errors
///
/// 写入失败时返回错误。
async fn netboot(harness: &Harness) -> Outcome<i64> {
    let app = harness.app()?;
    let dir = harness.data_dir().join("assets");
    let (linux, _linux_path) = store_bytes(&dir, LINUX).await?;
    let (initrd, _initrd_path) = store_bytes(&dir, INITRD).await?;
    let (archive, _archive_path) = store_bytes(&dir, b"e2e-netboot-archive").await?;
    let asset = VerifiedAsset {
        kind: AssetKind::DebianNetbootAmd64,
        label: "e2e netboot".to_owned(),
        source: AssetSource::Upload,
        source_url: None,
        sha256: archive,
        size: 19,
        kernel_abi: Some(KERNEL_ABI.to_owned()),
        verify: json!({ "files": { "linux": { "sha256": linux }, "initrd.gz": { "sha256": initrd } } }),
    };
    let actor = Actor {
        user_id: None,
        name: "e2e".to_owned(),
        role: Role::Admin,
        ip: "127.0.0.1".to_owned(),
    };
    let record = Catalog::new(app.store(), &dir).register(
        &asset,
        Operation {
            actor: &actor,
            now: app.now(),
        },
    )?;
    Ok(record.id)
}

/// 上传并激活全部引导资产。
///
/// # Errors
///
/// 任一步失败时返回错误。
pub async fn assets(harness: &Harness, login: &Login) -> Outcome<()> {
    for (kind, fixture) in [
        (AssetKind::IpxeUndionly, "ipxe/undionly.kpxe"),
        (AssetKind::IpxeEfiX64, "ipxe/snponly.efi"),
        (AssetKind::IpxeSnponlyX64, "ipxe/snponly.efi"),
    ] {
        let id = upload(harness, login, kind, fixture).await?;
        activate(harness, login, kind, id).await?;
    }
    let netboot_id = netboot(harness).await?;
    activate(harness, login, AssetKind::DebianNetbootAmd64, netboot_id).await
}

/// 以 bootnet 身份上报一次健康心跳。
///
/// # Errors
///
/// 请求失败时返回错误。
pub async fn heartbeat(harness: &Harness) -> Outcome<()> {
    let beat = Heartbeat {
        at: harness.app()?.now(),
        version: "e2e".to_owned(),
        rev: 0,
        mode: "proxy".to_owned(),
        health: Health::Ok,
        detail: None,
        listening: vec![67, 69, 4011],
        counters: xpxe_domain::Counters::default(),
        probe: None,
    };
    let reply = harness
        .internal(
            "POST",
            "/internal/v1/bootnet/heartbeat",
            Some(&serde_json::to_value(beat)?),
        )
        .await?;
    if (200..300).contains(&reply.status) {
        Ok(())
    } else {
        Err(format!("心跳 → {}：{}", reply.status, reply.text()).into())
    }
}

/// 登记节点，返回 ID。
///
/// # Errors
///
/// 请求失败时返回错误。
pub async fn register(harness: &Harness, login: &Login, spec: &Value, ids: Ids) -> Outcome<i64> {
    let body = json!({ "spec": spec, "profile_id": ids.profile, "init_plan_id": ids.plan });
    let node = harness
        .ok("POST", "/api/v1/nodes", login, Some(&body))
        .await?;
    node.get("id")
        .and_then(Value::as_i64)
        .ok_or_else(|| TestFailure::new(format!("登记未返回 id：{node}")).into())
}

/// LAN 节点规格。
#[must_use]
pub fn lan_spec(hostname: &str, mac: &str, firmware: &str) -> Value {
    json!({
        "hostname": hostname, "fqdn": null, "mac": mac, "os_family": "debian13",
        "firmware": firmware, "channel": { "type": "lan" }, "network": { "type": "dhcp" },
    })
}

/// 完整 LAN 准备：管理员登录、配置、资产、心跳。
///
/// # Errors
///
/// 任一步失败时返回错误。
pub async fn lan(harness: &Harness) -> Outcome<(Login, Ids)> {
    let login = harness.admin_login().await?;
    let ids = configs(harness, &login, &DhcpRules::standard()).await?;
    assets(harness, &login).await?;
    heartbeat(harness).await?;
    Ok((login, ids))
}
