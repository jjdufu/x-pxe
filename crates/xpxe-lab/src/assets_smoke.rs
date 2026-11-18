//! `assets-smoke` / `assets-align`：联网获取并校验资产、镜像内核 ABI 对齐检查（Step 7）。
//!
//! - `assets-smoke --data <dir> --master-key-file <file> --fetch <kind>… [--mirror <url>] [--suite trixie] [--offline]`
//! - `assets-align --mirror <url> [--suite trixie] [--unsigned] (--kernel-file <bzImage> | --data <dir> --master-key-file <file>)`

use std::path::{Path, PathBuf};

use serde_json::json;
use xpxe_assets::{
    Actor, AssetKind, AssetsError, Catalog, FetchPolicy, Fetcher, Ingest, Mirror, Operation,
    Source, TrustStore, VerifiedAsset, kernel_version, parse_kind,
};
use xpxe_store::{MasterKey, Role, Store};

use crate::common::{Args, LabError, LabResult, emit};
use crate::store_smoke::{ensure_key_file, now};

impl From<AssetsError> for LabError {
    fn from(value: AssetsError) -> Self {
        Self(value.to_string())
    }
}

/// 默认镜像。
const DEFAULT_MIRROR: &str = "http://deb.debian.org/debian";

/// 打开数据目录中的存储（主密钥文件不存在时生成）。
///
/// # Errors
///
/// 打开失败时返回错误。
fn open_store(args: &Args) -> LabResult<(Store, PathBuf)> {
    let data = PathBuf::from(args.required("--data")?);
    let key_path = PathBuf::from(args.required("--master-key-file")?);
    let _generated = ensure_key_file(&key_path)?;
    let (key, _warnings): (MasterKey, Vec<String>) = MasterKey::load(&key_path)?;
    let store = Store::open(&data, key)?;
    let assets = data.join("assets");
    std::fs::create_dir_all(&assets)?;
    Ok((store, assets))
}

/// 新建运行时。
///
/// # Errors
///
/// 创建失败时返回错误。
fn runtime() -> LabResult<tokio::runtime::Runtime> {
    Ok(tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?)
}

/// 系统管理员身份（实验室命令）。
fn lab_actor() -> Actor {
    Actor {
        user_id: None,
        name: "xpxe-lab".to_owned(),
        role: Role::Admin,
        ip: "127.0.0.1".to_owned(),
    }
}

/// `assets-smoke` 入口。
///
/// # Errors
///
/// 任一资产获取或校验失败时返回错误。
pub(crate) fn run_smoke(args: &Args) -> LabResult {
    let (store, dir) = open_store(args)?;
    let kinds = args
        .values("--fetch")
        .iter()
        .map(|name| parse_kind(name))
        .collect::<Result<Vec<_>, _>>()?;
    if kinds.is_empty() {
        return Err(LabError("至少需要一个 --fetch <kind>".to_owned()));
    }
    let mirror_url = args
        .value("--mirror")
        .unwrap_or_else(|| DEFAULT_MIRROR.to_owned());
    let suite = args.value("--suite").unwrap_or_else(|| "trixie".to_owned());
    let mirror = Mirror {
        url: &mirror_url,
        suite: &suite,
        signed: !args.flag("--unsigned"),
    };
    let fetcher = Fetcher::new(FetchPolicy::standard(args.flag("--offline")));
    let trust = TrustStore::debian()?;
    let ingest = Ingest {
        fetcher: &fetcher,
        trust: &trust,
        dir: &dir,
    };
    let actor = lab_actor();
    let catalog = Catalog::new(&store, &dir);
    let rt = runtime()?;
    for kind in kinds {
        let started = now()?;
        let op = Operation {
            actor: &actor,
            now: started,
        };
        let outcome = rt.block_on(fetch_one(ingest, kind, mirror, started));
        let asset = match outcome {
            Ok(asset) => asset,
            Err(error) => {
                catalog.record_failure(kind, None, &error, op)?;
                return Err(error.into());
            }
        };
        let record = catalog.register(&asset, op)?;
        emit(&json!({
            "step": "asset", "id": record.id, "kind": kind, "stage": record.stage,
            "sha256": record.meta.sha256, "size": record.meta.size, "source_url": record.meta.source_url,
            "kernel_abi": record.meta.kernel_abi, "verify": record.meta.verify,
        }))?;
        if let (AssetKind::DebianNetbootAmd64, Some(abi)) = (kind, asset.kernel_abi.as_deref()) {
            let report = rt.block_on(ingest.alignment(mirror, abi, started))?;
            emit(&json!({ "step": "alignment", "report": report }))?;
        }
    }
    let report = store.audit().verify()?;
    emit(
        &json!({ "step": "audit", "checked": report.checked, "first_broken": report.first_broken }),
    )
}

/// 获取一种资产。
///
/// # Errors
///
/// 获取或校验失败时返回错误。
async fn fetch_one(
    ingest: Ingest<'_>,
    kind: AssetKind,
    mirror: Mirror<'_>,
    at: xpxe_domain::Timestamp,
) -> Result<VerifiedAsset, AssetsError> {
    match kind {
        AssetKind::IpxeUndionly | AssetKind::IpxeEfiX64 | AssetKind::IpxeSnponlyX64 => {
            ingest.ipxe(kind, Source::Official).await
        }
        AssetKind::DebianNetbootAmd64 => ingest.netboot(mirror, at).await,
        AssetKind::DebianFirmwareCpio => ingest.firmware(Source::Official, at).await,
    }
}

/// `assets-align` 入口：已对齐返回成功；未对齐时输出镜像现有 ABI 并以失败退出。
///
/// # Errors
///
/// 检查失败或未对齐时返回错误。
pub(crate) fn run_align(args: &Args) -> LabResult {
    let mirror_url = args.required("--mirror")?;
    let suite = args.value("--suite").unwrap_or_else(|| "trixie".to_owned());
    let mirror = Mirror {
        url: &mirror_url,
        suite: &suite,
        signed: !args.flag("--unsigned"),
    };
    let abi = match args.value("--kernel-file") {
        Some(path) => abi_from_file(Path::new(&path))?,
        None => latest_netboot_abi(args)?,
    };
    let fetcher = Fetcher::new(FetchPolicy::standard(args.flag("--offline")));
    let trust = TrustStore::debian()?;
    let scratch = std::env::temp_dir();
    let ingest = Ingest {
        fetcher: &fetcher,
        trust: &trust,
        dir: &scratch,
    };
    let report = runtime()?.block_on(ingest.alignment(mirror, &abi, now()?))?;
    emit(&json!({ "step": "alignment", "abi": abi, "report": report }))?;
    if report.alignment.is_aligned() {
        Ok(())
    } else {
        Err(LabError(format!("未对齐：netboot ABI {abi} 不在镜像中")))
    }
}

/// 从 bzImage 文件读取 ABI。
///
/// # Errors
///
/// 读取或解析失败时返回错误。
fn abi_from_file(path: &Path) -> LabResult<String> {
    let data = std::fs::read(path)?;
    Ok(kernel_version(data.get(..data.len().min(64 * 1024)).unwrap_or_default())?.abi)
}

/// 数据目录中最新 netboot 资产的 ABI。
///
/// # Errors
///
/// 没有 netboot 资产时返回错误。
fn latest_netboot_abi(args: &Args) -> LabResult<String> {
    let (store, _dir) = open_store(args)?;
    store
        .assets()
        .list()?
        .into_iter()
        .filter(|record| record.meta.kind == AssetKind::DebianNetbootAmd64.as_str())
        .find_map(|record| record.meta.kernel_abi)
        .ok_or_else(|| LabError("数据目录中没有 netboot 资产，请用 --kernel-file 指定".to_owned()))
}
