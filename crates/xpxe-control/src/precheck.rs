//! arm 前置检查（Step 10 §3.1）：任一失败返回 `ARM_PRECHECK_FAILED` 与字段级错误。

use std::time::Duration;

use xpxe_assets::{AlignmentReport, AssetChannels, AssetKind, Catalog, Ingest, Mirror};
use xpxe_domain::init::TargetPlan;
use xpxe_domain::{
    BootChannel, BootNetwork, DhcpRules, FirmwareSet, Health, Profile, Sha256Hex, UefiFirmware,
};
use xpxe_store::{AssetRecord, NodeRow, VersionRef};

use crate::app::App;
use crate::bmc::bmc_by_name;
use crate::configs::singleton;
use crate::nodes::{NodeDoc, load};
use crate::{Failure, FieldIssue, Outcome, internal};

/// 对齐结论缓存时长。
const ALIGNMENT_TTL: u64 = 24 * 3600;
/// bootnet 心跳最长间隔。
const HEARTBEAT_MAX_AGE: u64 = 30;

/// 通过检查后的全部输入。
#[derive(Debug, Clone)]
pub(crate) struct Checked {
    /// 节点行。
    pub(crate) row: NodeRow,
    /// 节点规格对象。
    pub(crate) doc: NodeDoc,
    /// Profile。
    pub(crate) profile: (VersionRef, Profile),
    /// 初始化计划。
    pub(crate) plan: (VersionRef, TargetPlan),
    /// 资产通道版本。
    pub(crate) channels: Option<VersionRef>,
    /// iPXE 固件。
    pub(crate) firmware: FirmwareSet,
    /// netboot 资产。
    pub(crate) netboot: AssetRecord,
    /// 固件 cpio 资产。
    pub(crate) firmware_cpio: Option<AssetRecord>,
    /// agent 二进制 SHA-256。
    pub(crate) agent_sha256: String,
}

/// 收集到的问题。
type Issues = Vec<FieldIssue>;

/// 某种类的激活资产。
fn active(
    app: &App,
    channels: &AssetChannels,
    kind: AssetKind,
    issues: &mut Issues,
) -> Option<AssetRecord> {
    let Some(entry) = channels.channels.get(&kind) else {
        issues.push(FieldIssue::new(
            &format!("/assets/{kind}"),
            "NOT_ACTIVE",
            &format!("{kind} 尚未激活"),
        ));
        return None;
    };
    match app.store.assets().get(entry.asset_id) {
        Ok(record) => Some(record),
        Err(error) => {
            issues.push(FieldIssue::new(
                &format!("/assets/{kind}"),
                "MISSING",
                &error.to_string(),
            ));
            None
        }
    }
}

/// 摘要。
fn digest(record: Option<&AssetRecord>) -> Option<Sha256Hex> {
    Sha256Hex::parse(&record?.meta.sha256).ok()
}

/// 组装固件集合：缺失的 UEFI 固件用另一个 UEFI 固件代替（规则只会选其中之一）。
fn firmware_set(
    app: &App,
    channels: &AssetChannels,
    lan: bool,
    issues: &mut Issues,
) -> Option<FirmwareSet> {
    let rules = singleton::<DhcpRules>(&app.store)
        .ok()
        .flatten()
        .map_or_else(DhcpRules::standard, |(_, doc)| doc);
    let efi_kind = match rules.uefi_firmware() {
        UefiFirmware::IpxeEfi => AssetKind::IpxeEfiX64,
        UefiFirmware::SnpOnlyEfi => AssetKind::IpxeSnponlyX64,
    };
    let wanted_uefi = digest(
        active(
            app,
            channels,
            if lan { efi_kind } else { AssetKind::IpxeEfiX64 },
            issues,
        )
        .as_ref(),
    );
    let other = |kind: AssetKind| Sha256Hex::parse(&channels.channels.get(&kind)?.sha256).ok();
    let undionly = if lan {
        digest(active(app, channels, AssetKind::IpxeUndionly, issues).as_ref())
    } else {
        other(AssetKind::IpxeUndionly)
    };
    let uefi = wanted_uefi?;
    Some(FirmwareSet {
        undionly: undionly.unwrap_or_else(|| uefi.clone()),
        ipxe_efi: other(AssetKind::IpxeEfiX64).unwrap_or_else(|| uefi.clone()),
        snponly: other(AssetKind::IpxeSnponlyX64).unwrap_or_else(|| uefi.clone()),
    })
}

/// 镜像根地址。
fn mirror_url(profile: &Profile) -> String {
    let mirror = profile.mirror();
    format!(
        "{}://{}{}",
        mirror.scheme.as_str(),
        mirror.host_port.as_str(),
        mirror.directory.as_str()
    )
}

/// 对齐检查（缓存 24 h）。
async fn alignment(app: &App, netboot: &AssetRecord, profile: &Profile, issues: &mut Issues) {
    let path = "/assets/debian_netboot_amd64";
    let Some(abi) = netboot.meta.kernel_abi.clone() else {
        issues.push(FieldIssue::new(path, "NO_ABI", "netboot 资产缺少内核 ABI"));
        return;
    };
    let url = mirror_url(profile);
    let now = app.now();
    let key = (netboot.id, url.clone());
    let cached = app.volatile().alignment.get(&key).cloned();
    let fresh = cached
        .filter(|report| now.unix_seconds().saturating_sub(report.checked_at) < ALIGNMENT_TTL);
    let report: AlignmentReport = if let Some(report) = fresh {
        report
    } else {
        let ingest = Ingest {
            fetcher: &app.fetcher,
            trust: &app.trust,
            dir: &app.settings.assets_dir(),
        };
        let mirror = Mirror {
            url: &url,
            suite: "trixie",
            signed: true,
        };
        match tokio::time::timeout(Duration::from_secs(60), ingest.alignment(mirror, &abi, now))
            .await
        {
            Ok(Ok(report)) => {
                let _old = app.volatile().alignment.insert(key, report.clone());
                report
            }
            Ok(Err(error)) => {
                issues.push(FieldIssue::new(
                    path,
                    "ALIGNMENT_UNAVAILABLE",
                    &error.to_string(),
                ));
                return;
            }
            Err(_elapsed) => {
                issues.push(FieldIssue::new(
                    path,
                    "ALIGNMENT_UNAVAILABLE",
                    "镜像对齐检查超时",
                ));
                return;
            }
        }
    };
    if !report.alignment.is_aligned() {
        issues.push(FieldIssue::new(
            path,
            "ABI_MISMATCH",
            &format!("netboot 内核 ABI 与镜像源不一致：{:?}", report.alignment),
        ));
    }
}

/// 通道与引导网络模式、bootnet 健康。
///
/// # Errors
///
/// 失败时返回错误。
fn channel_mode(app: &App, doc: &NodeDoc, issues: &mut Issues) -> Outcome<()> {
    let network =
        singleton::<BootNetwork>(&app.store)?.map_or(BootNetwork::RedfishOnly, |(_, found)| found);
    match doc.spec.channel() {
        BootChannel::Lan => {
            if matches!(network, BootNetwork::RedfishOnly) {
                issues.push(FieldIssue::new(
                    "/boot_network",
                    "MODE_MISMATCH",
                    "LAN 节点要求引导网络模式为 ProxyDhcp 或 ExternalDhcp",
                ));
            }
        }
        BootChannel::Redfish { bmc } => match bmc_by_name(app, bmc.as_str()) {
            Ok((_version, found)) if found.pin.is_some() => {}
            Ok(_) => issues.push(FieldIssue::new(
                "/bmc",
                "BMC_NOT_PINNED",
                "BMC 证书指纹尚未固定",
            )),
            Err(error) => issues.push(FieldIssue::new("/bmc", "BMC_MISSING", error.message())),
        },
    }
    if matches!(network, BootNetwork::ProxyDhcp { .. })
        && matches!(doc.spec.channel(), BootChannel::Lan)
    {
        let beat = app.volatile().heartbeat.clone();
        match beat {
            Some(record) if matches!(record.heartbeat.health, Health::Conflict) => {
                issues.push(FieldIssue::new(
                    "/bootnet",
                    "BOOTNET_CONFLICT",
                    "bootnet 检测到现网 DHCP 冲突",
                ));
            }
            Some(record) if record.is_healthy(app.now(), HEARTBEAT_MAX_AGE) => {}
            Some(_) | None => issues.push(FieldIssue::new(
                "/bootnet",
                "BOOTNET_UNHEALTHY",
                "bootnet 不健康或无心跳",
            )),
        }
    }
    Ok(())
}

/// agent 二进制 SHA-256。
fn agent_digest(app: &App, issues: &mut Issues) -> String {
    match std::fs::read(&app.settings.agent_file) {
        Ok(bytes) => xpxe_render::init::sha256_hex(&bytes),
        Err(error) => {
            issues.push(FieldIssue::new(
                "/agent",
                "AGENT_MISSING",
                &format!("{}：{error}", app.settings.agent_file.display()),
            ));
            String::new()
        }
    }
}

/// 档案与计划的当前版本。
type Pair = ((VersionRef, Profile), (VersionRef, TargetPlan));

/// 配置校验（第 1 项）。
fn configs(app: &App, doc: &NodeDoc, issues: &mut Issues) -> Option<Pair> {
    let profile_head = app.store.configs().head::<Profile>(doc.profile_id);
    let plan_head = app.store.configs().head::<TargetPlan>(doc.init_plan_id);
    match (profile_head, plan_head) {
        (Ok(profile), Ok(plan)) => {
            if let Err(error) = xpxe_render::init::check_plan(&plan.1, &profile.1) {
                issues.push(FieldIssue::new("/init_plan", "INVALID", error.message()));
            }
            Some((profile, plan))
        }
        (profile_result, plan_result) => {
            if let Err(error) = profile_result {
                issues.push(FieldIssue::new("/profile", "INVALID", &error.to_string()));
            }
            if let Err(error) = plan_result {
                issues.push(FieldIssue::new("/init_plan", "INVALID", &error.to_string()));
            }
            None
        }
    }
}

/// 执行全部检查。
///
/// # Errors
///
/// 任一检查失败时返回 422 `ARM_PRECHECK_FAILED`。
pub(crate) async fn run(app: &App, node_id: i64) -> Outcome<Checked> {
    let (row, doc) = load(&app.store, node_id)?;
    let mut issues = Vec::new();
    let pair = configs(app, &doc, &mut issues);
    let (channels_ref, channels) = Catalog::new(&app.store, &app.settings.assets_dir())
        .channels()
        .map_err(internal)?;
    let lan = matches!(doc.spec.channel(), BootChannel::Lan);
    let firmware_found = firmware_set(app, &channels, lan, &mut issues);
    let netboot_found = active(app, &channels, AssetKind::DebianNetbootAmd64, &mut issues);
    let wants_cpio = pair
        .as_ref()
        .is_some_and(|((_, profile), _)| profile.firmware_cpio());
    let firmware_cpio = if wants_cpio {
        active(app, &channels, AssetKind::DebianFirmwareCpio, &mut issues)
    } else {
        None
    };
    if let (Some(asset), Some(((_, profile), _))) = (netboot_found.as_ref(), pair.as_ref()) {
        alignment(app, asset, profile, &mut issues).await;
    }
    channel_mode(app, &doc, &mut issues)?;
    let agent_sha256 = agent_digest(app, &mut issues);
    match (pair, firmware_found, netboot_found) {
        (Some((profile, plan)), Some(firmware), Some(netboot)) if issues.is_empty() => {
            Ok(Checked {
                row,
                doc,
                profile,
                plan,
                channels: channels_ref,
                firmware,
                netboot,
                firmware_cpio,
                agent_sha256,
            })
        }
        _ => Err(Failure::new(
            http::StatusCode::UNPROCESSABLE_ENTITY,
            "ARM_PRECHECK_FAILED",
            "arm 前置检查未通过",
        )
        .with_fields(issues)),
    }
}
