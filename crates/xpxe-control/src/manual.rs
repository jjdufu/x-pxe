//! 手动 ISO 网络引导（Step 14）：ISO 上传与库、抽取产物、引导端 `/m/*` 与菜单分流。
//!
//! 与节点状态机完全隔离：这里不产生任何状态转换，只写观测与审计。

mod extract;
mod library;
mod serve;
mod upload;

use std::io::{Read as _, Seek, Write};
use std::path::Path;

use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use xpxe_domain::{BootMenu, Sha256Hex};
use xpxe_image::iso_read::{IsoReader, detect};

use crate::app::App;
use crate::configs::singleton;
use crate::{Failure, Outcome};

pub(crate) use extract::{prepare, spawn_prepare, status};
pub(crate) use library::{delete, list, tree};
pub(crate) use serve::{menu_for_mac, preview, routes};
pub(crate) use upload::{abort, begin, chunk, complete, progress};

/// 当前菜单（不存在时为默认的关闭状态）。
///
/// # Errors
///
/// 读取失败时返回错误。
pub(crate) fn current_menu(app: &App) -> Outcome<BootMenu> {
    Ok(singleton::<BootMenu>(&app.store)?.map_or_else(BootMenu::default, |(_, menu)| menu))
}

/// 解析路径中的摘要。
///
/// # Errors
///
/// 不是 64 位十六进制时返回 404（不暴露更多信息）。
pub(crate) fn sha_param(raw: &str) -> Outcome<Sha256Hex> {
    Sha256Hex::parse(raw).map_err(|_error| Failure::not_found("ISO 不存在"))
}

/// ISO、抽取产物与进行中上传的合计字节数（配额口径）。
///
/// # Errors
///
/// 读取失败时返回错误。
pub(crate) fn usage(app: &App) -> Outcome<u64> {
    let isos = app.store.isos();
    let mut total = 0_u64;
    for iso in isos.list()? {
        total = total.saturating_add(iso.size);
        for extract in isos.extracts(&iso.sha256)? {
            total = total.saturating_add(extract.size);
        }
    }
    for upload in isos.uploads()? {
        total = total.saturating_add(upload.size);
    }
    Ok(total)
}

/// 边写边算 SHA-256 的写入器。
pub(crate) struct HashWriter<W> {
    /// 下游。
    inner: W,
    /// 摘要状态。
    hasher: Sha256,
    /// 已写字节数。
    written: u64,
}

impl<W: Write> HashWriter<W> {
    /// 包装下游写入器。
    pub(crate) fn new(inner: W) -> Self {
        Self {
            inner,
            hasher: Sha256::new(),
            written: 0,
        }
    }

    /// 结束：返回（十六进制摘要，字节数，下游）。
    pub(crate) fn finish(self) -> (String, u64, W) {
        (
            hex::encode(self.hasher.finalize()),
            self.written,
            self.inner,
        )
    }
}

impl<W: Write> Write for HashWriter<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let count = self.inner.write(buf)?;
        let done = buf.get(..count).unwrap_or_default();
        self.hasher.update(done);
        self.written = self
            .written
            .saturating_add(u64::try_from(count).unwrap_or(u64::MAX));
        Ok(count)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

/// 流式计算文件 SHA-256。
///
/// # Errors
///
/// 读失败时返回错误。
pub(crate) fn hash_file(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(buffer.get(..read).unwrap_or_default());
    }
    Ok(hex::encode(hasher.finalize()))
}

/// 识别 ISO：卷信息、文件名来源与发行版特征。解析失败只标记“未识别”。
#[must_use]
pub(crate) fn inspect<R: std::io::Read + Seek>(inner: R) -> Value {
    let mut reader = match IsoReader::open(inner) {
        Ok(reader) => reader,
        Err(error) => {
            return json!({ "family": "unknown", "volume_id": "", "hybrid": false, "error": error.to_string() });
        }
    };
    let info = reader.info().clone();
    let base = json!({
        "volume_id": info.volume_id,
        "size": info.size,
        "hybrid": info.boot.hybrid,
        "el_torito": info.boot.el_torito,
        "joliet": info.names.joliet,
        "rock_ridge": info.names.rock_ridge,
    });
    let detail = match reader.walk() {
        Ok(entries) => {
            let found = detect(&entries);
            json!({
                "family": found.family.as_str(),
                "kernel_path": found.kernel_path,
                "initrd_path": found.initrd_path,
                "embed_iso": found.embed_iso,
                "iso_name": found.iso_name,
                "recommended": if found.kernel_path.is_some() { "kernel_initrd" } else { "sanboot" },
            })
        }
        Err(error) => {
            json!({ "family": "unknown", "recommended": "sanboot", "error": error.to_string() })
        }
    };
    merge(base, detail)
}

/// 合并两个 JSON 对象（右侧覆盖）。
fn merge(mut left: Value, right: Value) -> Value {
    if let (Some(target), Value::Object(source)) = (left.as_object_mut(), right) {
        target.extend(source);
    }
    left
}

#[cfg(test)]
mod tests;
