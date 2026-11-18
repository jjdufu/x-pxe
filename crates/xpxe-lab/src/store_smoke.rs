//! `store-smoke`：建库、写配置、回滚、BMC 密码加解密、审计校验、备份并独立打开（Step 6）。
//!
//! 参数：`--data <dir>`、`--master-key-file <file>`（不存在时生成 32 字节原始密钥，权限 0400）。

use std::fs;
use std::io::Write as _;
use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::json;
use xpxe_domain::{Timeouts, Timestamp, Ttl};
use xpxe_store::{AuditRecord, Authorship, DB_FILE, MasterKey, Store, StoreError};

use crate::common::{Args, LabError, LabResult, emit};

impl From<StoreError> for LabError {
    fn from(value: StoreError) -> Self {
        Self(value.to_string())
    }
}

/// 当前时间。
///
/// # Errors
///
/// 系统时钟早于 1970 年时返回错误。
pub(crate) fn now() -> LabResult<Timestamp> {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| LabError(error.to_string()))?
        .as_secs();
    Ok(Timestamp::from_unix_seconds(secs))
}

/// 主密钥文件不存在时生成。
///
/// # Errors
///
/// 写文件失败时返回错误。
pub(crate) fn ensure_key_file(path: &Path) -> LabResult<bool> {
    if path.exists() {
        return Ok(false);
    }
    let key = MasterKey::generate()?;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o400)
        .open(path)?;
    file.write_all(key.to_base64().as_bytes())?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o400))?;
    Ok(true)
}

/// 子命令入口。
///
/// # Errors
///
/// 任一步骤失败时返回错误。
pub(crate) fn run(args: &Args) -> LabResult {
    let data = std::path::PathBuf::from(args.required("--data")?);
    let key_path = std::path::PathBuf::from(args.required("--master-key-file")?);
    let generated = ensure_key_file(&key_path)?;
    let (key, warnings) = MasterKey::load(&key_path)?;
    emit(
        &json!({"step": "master_key", "generated": generated, "key_id": key.key_id(), "warnings": warnings}),
    )?;
    let store = Store::open(&data, key.clone())?;
    emit(&json!({"step": "open", "db": store.path(), "schema": store.schema_version()?}))?;
    let started = now()?;
    let (object_id, name) = config_step(&store, started)?;
    store.secrets().put(object_id, "smoke-bmc-password")?;
    let secret_ok = store.secrets().get(object_id)?.as_deref() == Some("smoke-bmc-password");
    emit(&json!({"step": "secret", "roundtrip": secret_ok}))?;
    if !secret_ok {
        return Err(LabError("密文往返失败".to_owned()));
    }
    audit_step(&store, &name, object_id, started)?;
    backup_step(&store, &data, key, started)?;
    emit(&json!({"step": "done", "ok": true}))
}

/// 建配置对象、改一版、回滚并比较差异，返回 `(对象 ID, 名称)`。
///
/// # Errors
///
/// 写入失败或回滚内容不一致时返回错误。
fn config_step(store: &Store, started: Timestamp) -> LabResult<(i64, String)> {
    let meta = |comment: &str| Authorship {
        author_id: None,
        comment: comment.to_owned(),
        at: started,
    };
    let configs = store.configs();
    let base = Timeouts::from_ttl(Ttl::standard());
    let existing = configs.list(xpxe_domain::ConfigKind::Timeouts)?.len();
    let name = format!("store-smoke-{}-{existing}", started.unix_seconds());
    let first = configs.create(&name, &base, &meta("store-smoke 初版"))?;
    let changed = Timeouts { arm: 600, ..base };
    let second = configs.save(first.object_id, &changed, first.version, &meta("缩短 arm"))?;
    let rolled = configs.rollback::<Timeouts>(
        first.object_id,
        first.version,
        second.version,
        &meta("smoke"),
    )?;
    let diff = configs.diff::<Timeouts>(first.object_id, second.version, rolled.version)?;
    let restored_ok = rolled.content_sha256 == first.content_sha256;
    let paths: Vec<String> = diff.into_iter().map(|change| change.path).collect();
    emit(
        &json!({"step": "config", "object_id": first.object_id, "versions": [first.version, second.version, rolled.version], "rollback_restores_v1": restored_ok, "diff_paths": paths}),
    )?;
    if restored_ok {
        Ok((first.object_id, name))
    } else {
        Err(LabError("回滚内容与版本 1 不一致".to_owned()))
    }
}

/// 追加审计并校验链。
///
/// # Errors
///
/// 写入失败或链断裂时返回错误。
fn audit_step(store: &Store, name: &str, object_id: i64, started: Timestamp) -> LabResult {
    for action in ["config.create", "config.save", "config.rollback"] {
        let _audit_id = store.audit().append(
            &AuditRecord {
                actor: "store-smoke".to_owned(),
                ip: "127.0.0.1".to_owned(),
                action: action.to_owned(),
                target: name.to_owned(),
                outcome: "ok".to_owned(),
                detail: json!({ "object_id": object_id }),
            },
            started,
        )?;
    }
    let report = store.audit().verify()?;
    emit(
        &json!({"step": "audit", "checked": report.checked, "first_broken": report.first_broken, "ok": report.first_broken.is_none()}),
    )?;
    if report.first_broken.is_some() {
        return Err(LabError("审计链断裂".to_owned()));
    }
    Ok(())
}

/// 在线备份，并把备份放进临时目录用同一主密钥独立打开校验。
///
/// # Errors
///
/// 备份、打开或校验失败时返回错误。
fn backup_step(store: &Store, data: &Path, key: MasterKey, started: Timestamp) -> LabResult {
    let backups = data.join("backups");
    fs::create_dir_all(&backups)?;
    let backup_file = backups.join(format!("xpxe-{}.db", started.unix_seconds()));
    store.backup(&backup_file)?;
    let check_dir = backups.join(format!("restore-check-{}", started.unix_seconds()));
    fs::create_dir_all(&check_dir)?;
    let _copied = fs::copy(&backup_file, check_dir.join(DB_FILE))?;
    let copy = Store::open(&check_dir, key)?;
    let integrity = copy.integrity_check()?;
    let copy_chain = copy.audit().verify()?;
    drop(copy);
    fs::remove_dir_all(&check_dir)?;
    let healthy = integrity.is_empty() && copy_chain.first_broken.is_none();
    emit(
        &json!({"step": "backup", "file": backup_file, "independent_open": true, "integrity": integrity, "audit_ok": copy_chain.first_broken.is_none(), "audit_checked": copy_chain.checked}),
    )?;
    if healthy {
        Ok(())
    } else {
        Err(LabError("备份校验失败".to_owned()))
    }
}
