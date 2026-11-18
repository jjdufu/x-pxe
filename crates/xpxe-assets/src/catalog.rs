//! 资产目录：登记已校验资产、金丝雀、激活（仅 admin）、按种类回滚、受引用保护的删除。

use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{Value, json};
use xpxe_domain::{ConfigKind, Timestamp};
use xpxe_store::{
    AssetRecord, AssetSource, AssetStage, AuditRecord, Authorship, NewAsset, Role, Store,
    StoreError, VersionRef,
};

use crate::error::{AssetsError, AssetsResult, fail};
use crate::kind::{AssetChannels, AssetKind, ChannelEntry};

/// 资产频道配置对象名称。
pub const CHANNELS_OBJECT: &str = "asset_channels";

/// 操作者。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Actor {
    /// 用户 ID（系统操作为空）。
    pub user_id: Option<i64>,
    /// 用户名。
    pub name: String,
    /// 角色。
    pub role: Role,
    /// 来源 IP。
    pub ip: String,
}

/// 一次操作的上下文：操作者与时间。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Operation<'actor> {
    /// 操作者。
    pub actor: &'actor Actor,
    /// 操作时间。
    pub now: Timestamp,
}

/// 金丝雀结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanaryRun {
    /// 节点 ID。
    pub node_id: i64,
    /// 会话 ID。
    pub session_id: String,
}

/// 一条审计事件。
#[derive(Debug, Clone, PartialEq)]
struct Event<'text> {
    /// 动作。
    action: &'text str,
    /// 目标。
    target: String,
    /// 结果。
    outcome: &'text str,
    /// 详情。
    detail: Value,
}

/// 校验通过、待登记的资产。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VerifiedAsset {
    /// 种类。
    pub kind: AssetKind,
    /// 标签。
    pub label: String,
    /// 来源。
    pub source: AssetSource,
    /// 最终 URL。
    pub source_url: Option<String>,
    /// 资产文件（netboot 为清单）SHA-256。
    pub sha256: String,
    /// 字节数。
    pub size: u64,
    /// 内核 ABI（netboot）。
    pub kernel_abi: Option<String>,
    /// 校验结论（签名者、版本串、`ETag` 等）。
    pub verify: Value,
}

/// 资产目录。
#[derive(Debug, Clone, Copy)]
pub struct Catalog<'store> {
    /// 存储。
    store: &'store Store,
    /// 资产目录（`/data/assets`）。
    dir: &'store Path,
}

impl<'store> Catalog<'store> {
    /// 构造。
    #[must_use]
    pub const fn new(store: &'store Store, dir: &'store Path) -> Self {
        Self { store, dir }
    }

    /// 内容寻址路径。
    #[must_use]
    pub fn path_of(self, sha256: &str) -> PathBuf {
        self.dir.join(sha256)
    }

    /// 登记已校验资产（阶段 `verified`）并写审计。
    ///
    /// # Errors
    ///
    /// 写入失败时返回错误。
    pub fn register(self, asset: &VerifiedAsset, op: Operation<'_>) -> AssetsResult<AssetRecord> {
        let new = NewAsset {
            kind: asset.kind.as_str().to_owned(),
            label: asset.label.clone(),
            source: asset.source,
            source_url: asset.source_url.clone(),
            sha256: asset.sha256.clone(),
            size: asset.size,
            verify: asset.verify.clone(),
            kernel_abi: asset.kernel_abi.clone(),
            created_by: op.actor.name.clone(),
        };
        let assets = self.store.assets();
        let id = assets
            .insert(&new, op.now)
            .map_err(|error| store_error(&error))?;
        assets
            .set_stage(id, AssetStage::Verified, None)
            .map_err(|error| store_error(&error))?;
        self.audit(
            op,
            Event {
                action: "asset.verified",
                target: format!("asset:{id}"),
                outcome: "ok",
                detail: json!({
                    "kind": asset.kind, "sha256": asset.sha256, "source": asset.source,
                    "source_url": asset.source_url,
                }),
            },
        )?;
        assets.get(id).map_err(|error| store_error(&error))
    }

    /// 记录校验失败（文件已删除，只留审计记录与原因）。
    ///
    /// # Errors
    ///
    /// 写审计失败时返回错误。
    pub fn record_failure(
        self,
        kind: AssetKind,
        source_url: Option<&str>,
        error: &AssetsError,
        op: Operation<'_>,
    ) -> AssetsResult<()> {
        self.audit(
            op,
            Event {
                action: "asset.verify_failed",
                target: kind.as_str().to_owned(),
                outcome: "error",
                detail: json!({
                    "source_url": source_url, "code": error.code(), "reason": error.message(),
                }),
            },
        )
    }

    /// 当前频道头版本与内容；尚未创建时返回空频道。
    ///
    /// # Errors
    ///
    /// 读取失败时返回错误。
    pub fn channels(self) -> AssetsResult<(Option<VersionRef>, AssetChannels)> {
        let Some(object_id) = self.channel_object()? else {
            return Ok((None, AssetChannels::default()));
        };
        let (head, doc) = self
            .store
            .configs()
            .head::<AssetChannels>(object_id)
            .map_err(|error| store_error(&error))?;
        Ok((Some(head), doc))
    }

    /// 金丝雀会话到达 `Ready`：资产标记为 `canary_passed` 并记录节点与会话。
    ///
    /// # Errors
    ///
    /// 资产不存在或不处于 `verified`/`canary_passed` 时返回错误。
    pub fn mark_canary_passed(
        self,
        asset_id: i64,
        run: &CanaryRun,
        op: Operation<'_>,
    ) -> AssetsResult<()> {
        let record = self
            .store
            .assets()
            .get(asset_id)
            .map_err(|error| store_error(&error))?;
        if !matches!(
            record.stage,
            AssetStage::Verified | AssetStage::CanaryPassed
        ) {
            return Err(fail("bad_stage", "只有 verified 资产可以通过金丝雀"));
        }
        let mut verify = record.meta.verify;
        let canary = json!({
            "node_id": run.node_id, "session_id": run.session_id, "at": op.now.unix_seconds(),
        });
        if let Some(map) = verify.as_object_mut() {
            let _previous = map.insert("canary".to_owned(), canary.clone());
        }
        self.store
            .assets()
            .set_stage(asset_id, AssetStage::CanaryPassed, Some(&verify))
            .map_err(|error| store_error(&error))?;
        self.audit(
            op,
            Event {
                action: "asset.canary_passed",
                target: format!("asset:{asset_id}"),
                outcome: "ok",
                detail: canary,
            },
        )
    }

    /// 激活：仅 admin；资产须为 `canary_passed`，或附带跳过金丝雀的书面理由（写审计）。
    ///
    /// # Errors
    ///
    /// 权限不足（`forbidden`）、种类不符、未通过金丝雀且无理由（`canary_required`）或写入失败时返回错误。
    pub fn activate(
        self,
        kind: AssetKind,
        asset_id: i64,
        skip_canary_reason: Option<&str>,
        op: Operation<'_>,
    ) -> AssetsResult<VersionRef> {
        if op.actor.role != Role::Admin {
            self.audit(
                op,
                Event {
                    action: "asset.activate",
                    target: format!("asset:{asset_id}"),
                    outcome: "denied",
                    detail: json!({ "kind": kind }),
                },
            )?;
            return Err(fail("forbidden", "只有 admin 可以激活资产"));
        }
        let record = self
            .store
            .assets()
            .get(asset_id)
            .map_err(|error| store_error(&error))?;
        let reason = skip_canary_reason
            .map(str::trim)
            .filter(|text| !text.is_empty());
        check_activation(&record, kind, reason)?;
        let (head, mut doc) = self.channels()?;
        let entry = ChannelEntry {
            asset_id,
            sha256: record.meta.sha256.clone(),
        };
        let _previous = doc.channels.insert(kind, entry);
        let comment = reason.map_or_else(
            || format!("激活 {kind} → 资产 {asset_id}"),
            |text| format!("激活 {kind} → 资产 {asset_id}（跳过金丝雀：{text}）"),
        );
        let version = self.save(head.as_ref(), &doc, &comment, op)?;
        self.audit(
            op,
            Event {
                action: "asset.activate",
                target: format!("asset:{asset_id}"),
                outcome: "ok",
                detail: json!({
                    "kind": kind, "sha256": record.meta.sha256, "version": version.version,
                    "skip_canary_reason": reason,
                }),
            },
        )?;
        Ok(version)
    }

    /// 按种类回滚：把该种类恢复为版本链中上一个不同的值，生成新版本。
    ///
    /// # Errors
    ///
    /// 权限不足、没有可回滚的历史或写入失败时返回错误。
    pub fn rollback(self, kind: AssetKind, op: Operation<'_>) -> AssetsResult<VersionRef> {
        if op.actor.role != Role::Admin {
            return Err(fail("forbidden", "只有 admin 可以回滚资产频道"));
        }
        let (head, mut doc) = self.channels()?;
        let head_ref = head.ok_or_else(|| fail("no_history", "资产频道尚无版本"))?;
        let target = self.previous_value(&head_ref, kind, doc.channels.get(&kind))?;
        let comment = if let Some(entry) = &target {
            let _replaced = doc.channels.insert(kind, entry.clone());
            format!("回滚 {kind} → 资产 {}", entry.asset_id)
        } else {
            let _removed = doc.channels.remove(&kind);
            format!("回滚 {kind} → 未激活")
        };
        let version = self.save(Some(&head_ref), &doc, &comment, op)?;
        self.audit(
            op,
            Event {
                action: "asset.rollback",
                target: kind.as_str().to_owned(),
                outcome: "ok",
                detail: json!({
                    "version": version.version,
                    "asset_id": target.map(|entry| entry.asset_id),
                }),
            },
        )?;
        Ok(version)
    }

    /// 在版本链中向前查找该种类上一个不同的值。
    ///
    /// # Errors
    ///
    /// 没有不同的历史值（`no_history`）或读取失败时返回错误。
    fn previous_value(
        self,
        head: &VersionRef,
        kind: AssetKind,
        current: Option<&ChannelEntry>,
    ) -> AssetsResult<Option<ChannelEntry>> {
        let configs = self.store.configs();
        let history = configs
            .versions(head.object_id)
            .map_err(|error| store_error(&error))?;
        for info in history
            .into_iter()
            .filter(|info| info.version < head.version)
        {
            let old: AssetChannels = configs
                .load(head.object_id, info.version)
                .map_err(|error| store_error(&error))?;
            let value = old.channels.get(&kind);
            if value != current {
                return Ok(value.cloned());
            }
        }
        Err(AssetsError::new(
            "no_history",
            format!("{kind} 没有可回滚的上一个版本"),
        ))
    }

    /// 删除未被频道头版本与进行中会话引用的资产（含文件；netboot 成员文件在无其他引用时一并删除）。
    ///
    /// # Errors
    ///
    /// 被引用（`in_use`）、不存在或删除失败时返回错误。
    pub fn delete(self, asset_id: i64, op: Operation<'_>) -> AssetsResult<()> {
        let record = self
            .store
            .assets()
            .get(asset_id)
            .map_err(|error| store_error(&error))?;
        if self.referenced(asset_id, op.now)? {
            return Err(AssetsError::new(
                "in_use",
                format!("资产 {asset_id} 正被频道或进行中的会话引用"),
            ));
        }
        let others: Vec<AssetRecord> = self
            .store
            .assets()
            .list()
            .map_err(|error| store_error(&error))?
            .into_iter()
            .filter(|other| other.id != asset_id)
            .collect();
        let mut files = vec![record.meta.sha256.clone()];
        files.extend(member_hashes(&record.meta.verify));
        for sha in files {
            let shared = others.iter().any(|other| {
                other.meta.sha256 == sha || member_hashes(&other.meta.verify).contains(&sha)
            });
            if !shared {
                remove_if_exists(&self.path_of(&sha))?;
            }
        }
        self.store
            .assets()
            .delete(asset_id)
            .map_err(|error| store_error(&error))?;
        self.audit(
            op,
            Event {
                action: "asset.delete",
                target: format!("asset:{asset_id}"),
                outcome: "ok",
                detail: json!({ "kind": record.meta.kind, "sha256": record.meta.sha256 }),
            },
        )
    }

    /// 资产是否被频道头版本或未关闭会话引用。
    ///
    /// # Errors
    ///
    /// 读取失败时返回错误。
    pub fn referenced(self, asset_id: i64, now: Timestamp) -> AssetsResult<bool> {
        let (_head, doc) = self.channels()?;
        if doc.references(asset_id) {
            return Ok(true);
        }
        for pin in self
            .store
            .tokens()
            .open_pins(now)
            .map_err(|error| store_error(&error))?
        {
            let pinned_channels = match pin.asset_channels {
                Some(version) => self
                    .store
                    .configs()
                    .load::<AssetChannels>(version.object_id, version.version)
                    .map_err(|error| store_error(&error))?
                    .references(asset_id),
                None => false,
            };
            if pinned_channels || pin.canary_assets.contains(&asset_id) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// 频道对象 ID。
    ///
    /// # Errors
    ///
    /// 读取失败时返回错误。
    fn channel_object(self) -> AssetsResult<Option<i64>> {
        Ok(self
            .store
            .configs()
            .list(ConfigKind::AssetChannels)
            .map_err(|error| store_error(&error))?
            .into_iter()
            .find(|object| object.name == CHANNELS_OBJECT)
            .map(|object| object.id))
    }

    /// 保存频道新版本（首次时创建对象）。
    ///
    /// # Errors
    ///
    /// 冲突或写入失败时返回错误。
    fn save(
        self,
        head: Option<&VersionRef>,
        doc: &AssetChannels,
        comment: &str,
        op: Operation<'_>,
    ) -> AssetsResult<VersionRef> {
        let meta = Authorship {
            author_id: op.actor.user_id,
            comment: comment.to_owned(),
            at: op.now,
        };
        let configs = self.store.configs();
        head.map_or_else(
            || configs.create(CHANNELS_OBJECT, doc, &meta),
            |current| configs.save(current.object_id, doc, current.version, &meta),
        )
        .map_err(|error| store_error(&error))
    }

    /// 写审计。
    ///
    /// # Errors
    ///
    /// 写入失败时返回错误。
    fn audit(self, op: Operation<'_>, event: Event<'_>) -> AssetsResult<()> {
        let record = AuditRecord {
            actor: op.actor.name.clone(),
            ip: op.actor.ip.clone(),
            action: event.action.to_owned(),
            target: event.target,
            outcome: event.outcome.to_owned(),
            detail: event.detail,
        };
        self.store
            .audit()
            .append(&record, op.now)
            .map(|_id| ())
            .map_err(|error| store_error(&error))
    }
}

/// 激活前检查：种类一致，且已通过金丝雀或附带跳过理由。
///
/// # Errors
///
/// 种类不符（`kind_mismatch`）或未通过金丝雀且无理由（`canary_required`）时返回错误。
fn check_activation(
    record: &AssetRecord,
    kind: AssetKind,
    reason: Option<&str>,
) -> AssetsResult<()> {
    let asset_id = record.id;
    if record.meta.kind != kind.as_str() {
        return Err(AssetsError::new(
            "kind_mismatch",
            format!("资产 {asset_id} 的种类是 {}", record.meta.kind),
        ));
    }
    let allowed = match record.stage {
        AssetStage::CanaryPassed => true,
        AssetStage::Verified => reason.is_some(),
        AssetStage::Staged | AssetStage::Retired => false,
    };
    if !allowed {
        return Err(fail(
            "canary_required",
            "资产未通过金丝雀；跳过金丝雀必须填写理由",
        ));
    }
    Ok(())
}

/// netboot 清单成员的哈希（`verify.files.*.sha256`）。
fn member_hashes(verify: &Value) -> Vec<String> {
    verify
        .get("files")
        .and_then(Value::as_object)
        .map_or_else(Vec::new, |files| {
            files
                .values()
                .filter_map(|file| {
                    file.get("sha256")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                })
                .collect()
        })
}

/// 删除文件；不存在视为成功。
///
/// # Errors
///
/// 其他 I/O 错误时返回错误。
fn remove_if_exists(path: &Path) -> AssetsResult<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(AssetsError::new(
            "io",
            format!("{}：{error}", path.display()),
        )),
    }
}

/// 存储错误转换。
pub(crate) fn store_error(error: &StoreError) -> AssetsError {
    let code = match error {
        StoreError::Conflict { .. } => "conflict",
        StoreError::NotFound(_) => "not_found",
        StoreError::Sqlite(_)
        | StoreError::Io(_)
        | StoreError::Config(_)
        | StoreError::StateChanged { .. }
        | StoreError::MasterKey(_)
        | StoreError::KeyMismatch
        | StoreError::Crypto
        | StoreError::Poisoned
        | StoreError::Invalid(_)
        | StoreError::AlreadyServed
        | StoreError::Throttled { .. }
        | StoreError::InvalidCredentials
        | StoreError::Corrupt(_) => "store",
    };
    AssetsError::new(code, error.to_string())
}
