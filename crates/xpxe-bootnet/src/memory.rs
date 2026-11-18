//! 固件内存缓存与 TFTP 文件表。
//!
//! TFTP 只从内存表查找：`fw/<sha12>/<名称>`（`ProxyDHCP` 下发的路径）与别名
//! `undionly.kpxe`、`ipxe.efi`、`snponly.efi`（指向当前激活版本，供现网 DHCP 使用）。

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, PoisonError, RwLock};

use tokio::sync::watch;
use xpxe_domain::{DirectiveSnapshot, FirmwareName, FirmwareSet, Sha256Hex};
use xpxe_tftp::server::Source;

/// 摘要 → 固件内容。
type BlobMap = BTreeMap<Sha256Hex, Arc<[u8]>>;

/// 已校验的固件。
#[derive(Debug, Default)]
pub struct FirmwareCache {
    /// 摘要 → 内容。
    blobs: RwLock<BlobMap>,
}

impl FirmwareCache {
    /// 按摘要取。
    #[must_use]
    pub fn get(&self, digest: &Sha256Hex) -> Option<Arc<[u8]>> {
        self.blobs
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .get(digest)
            .cloned()
    }

    /// 放入已校验的固件。
    pub fn insert(&self, digest: Sha256Hex, bytes: Arc<[u8]>) {
        let _previous: Option<Arc<[u8]>> = self
            .blobs
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(digest, bytes);
    }

    /// `wanted` 中尚未缓存的摘要。
    #[must_use]
    pub fn missing(&self, wanted: &BTreeSet<Sha256Hex>) -> Vec<Sha256Hex> {
        let blobs = self.blobs.read().unwrap_or_else(PoisonError::into_inner);
        wanted
            .iter()
            .filter(|digest| !blobs.contains_key(*digest))
            .cloned()
            .collect()
    }

    /// 只保留 `wanted` 中的固件。
    pub fn retain(&self, wanted: &BTreeSet<Sha256Hex>) {
        self.blobs
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|digest, _bytes| wanted.contains(digest));
    }
}

/// 名称 → 固件种类。
fn firmware_name(name: &str) -> Option<FirmwareName> {
    [
        FirmwareName::UndionlyKpxe,
        FirmwareName::IpxeEfi,
        FirmwareName::SnpOnlyEfi,
    ]
    .into_iter()
    .find(|candidate| candidate.as_str() == name)
}

/// 在快照中按 TFTP 路径解析出固件摘要。
#[must_use]
pub fn resolve(snapshot: &DirectiveSnapshot, path: &str) -> Option<Sha256Hex> {
    if let Some(name) = firmware_name(path) {
        return snapshot
            .active_firmware
            .as_ref()
            .map(|set| set.digest(name).clone());
    }
    let rest = path.strip_prefix("fw/")?;
    let (short, file) = rest.split_once('/')?;
    let name = firmware_name(file)?;
    snapshot
        .directives
        .iter()
        .map(|entry| &entry.firmware)
        .chain(snapshot.active_firmware.iter())
        .map(|set: &FirmwareSet| set.digest(name))
        .find(|digest| digest.short12() == short)
        .cloned()
}

/// TFTP 文件来源：当前快照 + 固件缓存。
#[derive(Debug)]
pub struct TftpFiles {
    /// 固件缓存。
    cache: Arc<FirmwareCache>,
    /// 当前生效快照。
    snapshot: watch::Receiver<Arc<DirectiveSnapshot>>,
}

impl TftpFiles {
    /// 新建。
    #[must_use]
    pub const fn new(
        cache: Arc<FirmwareCache>,
        snapshot: watch::Receiver<Arc<DirectiveSnapshot>>,
    ) -> Self {
        Self { cache, snapshot }
    }
}

impl Source for TftpFiles {
    fn open(&self, path: &str) -> Option<Arc<[u8]>> {
        let snapshot = Arc::clone(&self.snapshot.borrow());
        let digest = resolve(&snapshot, path)?;
        self.cache.get(&digest)
    }
}

#[cfg(test)]
mod tests;
