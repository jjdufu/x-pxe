//! 会话产物：arm 时渲染并缓存到 `<data>/cache/sessions/<会话>/`（0600），引导端按令牌读取。

use std::io::Write as _;
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use xpxe_domain::{FirmwareSet, SessionId, Timestamp};

use crate::settings::Settings;
use crate::{Outcome, internal};

/// 引导令牌文件名。
pub(crate) const BOOT_TOKEN: &str = "boot_token";
/// agent 令牌文件名。
pub(crate) const AGENT_TOKEN: &str = "agent_token";
/// 元数据文件名。
pub(crate) const META: &str = "meta.json";

/// 会话元数据。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SessionMeta {
    /// 节点。
    pub(crate) node_id: i64,
    /// 通道：`lan` 或 `redfish`。
    pub(crate) channel: String,
    /// netboot 内核 SHA-256。
    pub(crate) linux: String,
    /// netboot initrd SHA-256。
    pub(crate) initrd: String,
    /// `firmware.cpio.gz` SHA-256。
    pub(crate) firmware_cpio: Option<String>,
    /// iPXE 固件。
    pub(crate) firmware: FirmwareSet,
    /// 会话过期时间。
    pub(crate) expires_at: Timestamp,
}

/// 会话目录。
fn dir(settings: &Settings, session: &str) -> PathBuf {
    settings.sessions_dir().join(session)
}

/// 写入一组文件（0600）。
///
/// # Errors
///
/// 目录或文件写入失败时返回错误。
pub(crate) fn write(
    settings: &Settings,
    session: &SessionId,
    files: &[(String, Vec<u8>)],
) -> Outcome<()> {
    let base = dir(settings, session.as_str());
    std::fs::create_dir_all(&base).map_err(internal)?;
    for (name, bytes) in files {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(base.join(name))
            .map_err(internal)?;
        file.write_all(bytes).map_err(internal)?;
    }
    Ok(())
}

/// 读取一个文件；不存在返回 `None`。
///
/// # Errors
///
/// 读取失败（非“不存在”）时返回错误。
pub(crate) fn read(settings: &Settings, session: &str, name: &str) -> Outcome<Option<Vec<u8>>> {
    match std::fs::read(dir(settings, session).join(name)) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(internal(error)),
    }
}

/// 读取文本文件。
///
/// # Errors
///
/// 读取失败或不是 UTF-8 时返回错误。
pub(crate) fn read_text(settings: &Settings, session: &str, name: &str) -> Outcome<Option<String>> {
    read(settings, session, name)?
        .map(|bytes| String::from_utf8(bytes).map_err(internal))
        .transpose()
}

/// 读取元数据。
///
/// # Errors
///
/// 读取或解析失败时返回错误。
pub(crate) fn meta(settings: &Settings, session: &str) -> Outcome<Option<SessionMeta>> {
    read(settings, session, META)?
        .map(|bytes| serde_json::from_slice(&bytes).map_err(internal))
        .transpose()
}

/// 会话关闭：删除令牌原文（数据库侧已作废令牌）。
pub(crate) fn forget_tokens(settings: &Settings, session: &str) {
    for name in [BOOT_TOKEN, AGENT_TOKEN] {
        let path = dir(settings, session).join(name);
        if let Err(error) = std::fs::remove_file(&path)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!(%error, path = %path.display(), "删除令牌文件失败");
        }
    }
}

/// 删除整个会话目录（arm 失败回滚时）。
pub(crate) fn purge(settings: &Settings, session: &str) {
    let path = dir(settings, session);
    if let Err(error) = std::fs::remove_dir_all(&path)
        && error.kind() != std::io::ErrorKind::NotFound
    {
        tracing::warn!(%error, path = %path.display(), "删除会话目录失败");
    }
}
