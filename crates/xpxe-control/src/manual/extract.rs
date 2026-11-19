//! 抽取产物：从 ISO 中取出内核与 initrd，PVE 系再把整份 ISO 以 newc cpio 段追加进 initrd。
//!
//! 产物流式写入临时文件、边写边算 SHA-256，再原子改名为摘要；同一 (ISO, 种类, 路径, 嵌入名) 只生成一次。
//! 菜单保存后与启动时在后台补齐；进行中/失败状态只在内存中。

use std::io::{BufWriter, Write as _};
use std::path::{Path as FsPath, PathBuf};
use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use xpxe_domain::{BootMenuEntry, EntryKind};
use xpxe_image::iso_read::IsoReader;
use xpxe_render::manual::MenuAction;
use xpxe_store::{ExtractKey, ExtractRecord};

use crate::app::App;
use crate::auth::Principal;
use crate::{Outcome, internal};

/// 默认的嵌入 ISO 文件名（PVE 安装器在 initrd 根目录寻找）。
pub(crate) const DEFAULT_ISO_NAME: &str = "/proxmox.iso";
/// 内存中进行中的标记。
const RUNNING: &str = "running";

/// 某条 `kernel_initrd` 条目需要的两个产物（内核、initrd）。
fn keys_of(entry: &BootMenuEntry) -> Option<(ExtractKey, ExtractKey)> {
    let EntryKind::KernelInitrd {
        iso,
        kernel_path,
        initrd_path,
        embed_iso,
        iso_name,
        ..
    } = &entry.kind
    else {
        return None;
    };
    let kernel = ExtractKey {
        iso_sha256: iso.as_str().to_owned(),
        kind: "kernel".to_owned(),
        source_path: kernel_path.clone(),
        embed_name: String::new(),
    };
    let initrd = ExtractKey {
        iso_sha256: iso.as_str().to_owned(),
        kind: if *embed_iso {
            "initrd_with_iso"
        } else {
            "initrd"
        }
        .to_owned(),
        source_path: initrd_path.clone(),
        embed_name: if *embed_iso {
            iso_name
                .clone()
                .unwrap_or_else(|| DEFAULT_ISO_NAME.to_owned())
        } else {
            String::new()
        },
    };
    Some((kernel, initrd))
}

/// 内存状态用的键。
fn job_key(key: &ExtractKey) -> String {
    format!(
        "{}|{}|{}|{}",
        key.iso_sha256, key.kind, key.source_path, key.embed_name
    )
}

/// 已就绪的产物（记录存在且文件在盘上）。
///
/// # Errors
///
/// 读取失败时返回错误。
fn ready(app: &App, key: &ExtractKey) -> Outcome<Option<ExtractRecord>> {
    Ok(app.store.isos().extract(key)?.filter(|record| {
        app.settings
            .manual_extract_dir()
            .join(&record.sha256)
            .is_file()
    }))
}

/// 产物未就绪时的说明。
fn pending_reason(app: &App, key: &ExtractKey) -> String {
    match app.volatile().extracts.get(&job_key(key)) {
        Some(state) if state == RUNNING => "extract running".to_owned(),
        Some(error) => format!("extract failed: {error}"),
        None => "extract pending".to_owned(),
    }
}

/// 把菜单条目解析为可渲染的动作。
///
/// # Errors
///
/// 读取失败时返回错误。
pub(crate) fn action_for(app: &App, entry: &BootMenuEntry) -> Outcome<MenuAction> {
    if let Some(iso) = entry.kind.iso()
        && app.store.isos().live(iso.as_str())?.is_none()
    {
        return Ok(MenuAction::Unavailable {
            reason: "iso deleted".to_owned(),
        });
    }
    Ok(match &entry.kind {
        EntryKind::Sanboot { iso } => MenuAction::Sanboot {
            iso: iso.as_str().to_owned(),
        },
        EntryKind::KernelInitrd { cmdline, .. } => {
            let Some((kernel_key, initrd_key)) = keys_of(entry) else {
                return Ok(MenuAction::Exit);
            };
            match (ready(app, &kernel_key)?, ready(app, &initrd_key)?) {
                (Some(kernel), Some(initrd)) => MenuAction::Kernel {
                    kernel: kernel.sha256,
                    initrd: initrd.sha256,
                    cmdline: cmdline.clone(),
                },
                (None, _) => MenuAction::Unavailable {
                    reason: pending_reason(app, &kernel_key),
                },
                (Some(_), None) => MenuAction::Unavailable {
                    reason: pending_reason(app, &initrd_key),
                },
            }
        }
        EntryKind::Chain { url } => MenuAction::Chain { url: url.clone() },
        EntryKind::Exit => MenuAction::Exit,
    })
}

/// 生成一个产物：写临时文件 → fsync → 改名为摘要。
///
/// # Errors
///
/// ISO 读失败、路径不存在或写盘失败时返回说明。
fn produce(
    iso_path: &FsPath,
    key: &ExtractKey,
    dir: &FsPath,
    tmp_name: &str,
) -> Result<(String, u64), String> {
    let file = std::fs::File::open(iso_path).map_err(|error| error.to_string())?;
    let mut reader =
        IsoReader::open(std::io::BufReader::new(file)).map_err(|error| error.to_string())?;
    let entry = reader
        .find(&key.source_path)
        .map_err(|error| error.to_string())?
        .filter(|entry| !entry.is_dir)
        .ok_or_else(|| format!("ISO 中没有 {}", key.source_path))?;
    let tmp: PathBuf = dir.join(tmp_name);
    let written = write_artifact(&mut reader, &entry, key, &tmp);
    let (sha, size) = match written {
        Ok(done) => done,
        Err(error) => {
            let _gone = std::fs::remove_file(&tmp);
            return Err(error);
        }
    };
    std::fs::rename(&tmp, dir.join(&sha)).map_err(|error| error.to_string())?;
    Ok((sha, size))
}

/// 把产物写进临时文件并落盘（fsync）。
///
/// # Errors
///
/// 读 ISO 或写盘失败时返回说明。
fn write_artifact<R: std::io::Read + std::io::Seek>(
    reader: &mut IsoReader<R>,
    entry: &xpxe_image::iso_read::IsoEntry,
    key: &ExtractKey,
    tmp: &FsPath,
) -> Result<(String, u64), String> {
    let out = std::fs::File::create(tmp).map_err(|error| error.to_string())?;
    let mut writer = super::HashWriter::new(BufWriter::new(out));
    if key.embed_name.is_empty() {
        let _bytes = reader
            .copy_to(entry, &mut writer)
            .map_err(|error| error.to_string())?;
    } else {
        let _bytes = reader
            .write_initrd_with_iso(entry, &key.embed_name, &mut writer)
            .map_err(|error| error.to_string())?;
    }
    writer.flush().map_err(|error| error.to_string())?;
    let (sha, size, buffered) = writer.finish();
    let handle = buffered.into_inner().map_err(|error| error.to_string())?;
    handle.sync_all().map_err(|error| error.to_string())?;
    Ok((sha, size))
}

/// 生成一个产物并登记（在阻塞线程中运行）。
///
/// # Errors
///
/// 生成失败时返回说明。
async fn run_one(app: &Arc<App>, key: ExtractKey) -> Result<(), String> {
    let iso_path = app.settings.manual_iso_dir().join(&key.iso_sha256);
    let dir = app.settings.manual_extract_dir();
    let tmp_name = format!(
        ".tmp-{}",
        hex::encode(Sha256::digest(job_key(&key).as_bytes()))
    );
    let job = key.clone();
    let (sha, size) =
        tokio::task::spawn_blocking(move || produce(&iso_path, &job, &dir, &tmp_name))
            .await
            .map_err(|error| error.to_string())??;
    app.store
        .isos()
        .put_extract(
            &ExtractRecord {
                key,
                sha256: sha,
                size,
            },
            app.now(),
        )
        .map_err(|error| error.to_string())
}

/// 补齐当前菜单需要的全部产物（已就绪或进行中的跳过）。
pub(crate) async fn prepare(app: Arc<App>) {
    let menu = match super::current_menu(&app) {
        Ok(menu) => menu,
        Err(error) => {
            tracing::warn!(error = %error.message(), "读取手动引导菜单失败");
            return;
        }
    };
    let keys: Vec<ExtractKey> = menu
        .entries
        .iter()
        .filter(|entry| entry.enabled)
        .filter_map(keys_of)
        .flat_map(<[ExtractKey; 2]>::from)
        .collect();
    for key in keys {
        let name = job_key(&key);
        let live = app
            .store
            .isos()
            .live(&key.iso_sha256)
            .ok()
            .flatten()
            .is_some();
        if !live
            || ready(&app, &key).ok().flatten().is_some()
            || app
                .volatile()
                .extracts
                .get(&name)
                .is_some_and(|state| state == RUNNING)
        {
            continue;
        }
        let _old = app
            .volatile()
            .extracts
            .insert(name.clone(), RUNNING.to_owned());
        let outcome = run_one(&app, key).await;
        let mut volatile = app.volatile();
        match outcome {
            Ok(()) => {
                let _done = volatile.extracts.remove(&name);
                drop(volatile);
                app.push("iso.extracted", json!({ "job": name }));
            }
            Err(error) => {
                tracing::warn!(%error, job = %name, "ISO 抽取失败");
                let _failed = volatile.extracts.insert(name.clone(), error.clone());
                drop(volatile);
                app.push("iso.extract_failed", json!({ "job": name, "error": error }));
            }
        }
    }
}

/// 在后台补齐产物。
pub(crate) fn spawn_prepare(app: &Arc<App>) {
    drop(tokio::spawn(prepare(Arc::clone(app))));
}

/// `GET /boot-menu/status`：每个条目的可用性（未就绪的条目顺带触发补齐）。
///
/// # Errors
///
/// 读取失败时返回错误。
pub(crate) async fn status(
    State(app): State<Arc<App>>,
    _principal: Principal,
) -> Outcome<Json<Value>> {
    let menu = super::current_menu(&app)?;
    let mut items = Vec::new();
    let mut pending = false;
    for entry in &menu.entries {
        let action = action_for(&app, entry)?;
        pending |=
            matches!(&action, MenuAction::Unavailable { reason } if reason == "extract pending");
        items.push(json!({ "id": entry.id, "enabled": entry.enabled, "action": serde_json::to_value(&action).map_err(internal)? }));
    }
    if pending {
        spawn_prepare(&app);
    }
    Ok(Json(json!({ "enabled": menu.enabled, "entries": items })))
}
