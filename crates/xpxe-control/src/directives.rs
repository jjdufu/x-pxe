//! bootnet 指令快照：模式、规则、激活固件与 `Armed` 的 LAN 节点。

use xpxe_assets::{AssetChannels, AssetKind, Catalog};
use xpxe_domain::{
    BootNetwork, BootToken, DhcpRules, DirectiveSnapshot, FirmwareSet, MacAddr, NodeState,
    Sha256Hex, SnapshotEntry,
};

use crate::app::App;
use crate::artifacts::{self, BOOT_TOKEN};
use crate::configs::singleton;
use crate::{Outcome, internal};

/// 通道中某种类的摘要。
fn channel_digest(channels: &AssetChannels, kind: AssetKind) -> Option<Sha256Hex> {
    let entry = channels.channels.get(&kind)?;
    Sha256Hex::parse(&entry.sha256).ok()
}

/// 当前激活的固件集合（`undionly` 与 `ipxe.efi` 都激活时才有）。
pub(crate) fn active_firmware(channels: &AssetChannels) -> Option<FirmwareSet> {
    let undionly = channel_digest(channels, AssetKind::IpxeUndionly)?;
    let ipxe_efi = channel_digest(channels, AssetKind::IpxeEfiX64)?;
    let snponly =
        channel_digest(channels, AssetKind::IpxeSnponlyX64).unwrap_or_else(|| ipxe_efi.clone());
    Some(FirmwareSet {
        undionly,
        ipxe_efi,
        snponly,
    })
}

/// 当前通道。
///
/// # Errors
///
/// 读取失败时返回错误。
pub(crate) fn channels(app: &App) -> Outcome<AssetChannels> {
    let (_head, doc) = Catalog::new(&app.store, &app.settings.assets_dir())
        .channels()
        .map_err(internal)?;
    Ok(doc)
}

/// `Armed` 的 LAN 节点条目。
///
/// # Errors
///
/// 读取失败时返回错误。
fn entries(app: &App) -> Outcome<Vec<SnapshotEntry>> {
    let mut out = Vec::new();
    for row in app.store.nodes().list()? {
        let Some(session) = row
            .active_session_id
            .clone()
            .filter(|_| row.state == NodeState::Armed)
        else {
            continue;
        };
        let Some(meta) = artifacts::meta(&app.settings, session.as_str())? else {
            continue;
        };
        let stored = artifacts::read_text(&app.settings, session.as_str(), BOOT_TOKEN)?;
        let Some(token) = stored.filter(|_| meta.channel == "lan") else {
            continue;
        };
        out.push(SnapshotEntry {
            mac: MacAddr::parse(&row.mac).map_err(internal)?,
            session,
            boot_token: BootToken::parse(token.trim()).map_err(internal)?,
            firmware: meta.firmware,
            uefi_firmware: None,
            expires_at: meta.expires_at,
        });
    }
    Ok(out)
}

/// 生成快照。
///
/// # Errors
///
/// 读取失败时返回错误。
pub(crate) fn snapshot(app: &App) -> Outcome<DirectiveSnapshot> {
    let rev = *app.rev.borrow();
    let network =
        singleton::<BootNetwork>(&app.store)?.map_or(BootNetwork::RedfishOnly, |(_, doc)| doc);
    let rules =
        singleton::<DhcpRules>(&app.store)?.map_or_else(DhcpRules::standard, |(_, doc)| doc);
    let mut snapshot = DirectiveSnapshot::closed(rev, network, app.settings.boot_base.clone());
    snapshot.rules = rules;
    snapshot.active_firmware = active_firmware(&channels(app)?);
    snapshot.directives = entries(app)?;
    Ok(snapshot)
}
