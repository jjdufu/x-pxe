//! Debian `Release`/`InRelease` 字段与 `SHA256SUMS` 解析。

use std::time::Duration;

use time::OffsetDateTime;
use time::format_description::well_known::Rfc2822;
use xpxe_domain::Timestamp;

use crate::error::{AssetsError, AssetsResult, fail};
use crate::trust::to_timestamp;

/// `InRelease` 过旧警告阈值（镜像可能滞后）。
pub const STALE_AFTER: Duration = Duration::from_hours(24 * 30);

/// `Release` 中 `SHA256:` 段的一项。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseEntry {
    /// 十六进制 SHA-256。
    pub sha256: String,
    /// 字节数。
    pub size: u64,
}

/// 解析后的 `Release`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    /// `Date` 字段。
    pub date: Option<Timestamp>,
    /// `Codename` 字段。
    pub codename: Option<String>,
    /// `SHA256:` 段：（相对路径，条目）。
    pub entries: Vec<(String, ReleaseEntry)>,
}

impl Release {
    /// 解析 `Release` 文本（内联签名已剥离）。
    ///
    /// # Errors
    ///
    /// `Date` 格式错误或 `SHA256:` 段条目格式错误时返回错误。
    pub fn parse(text: &str) -> AssetsResult<Self> {
        let mut date = None;
        let mut codename = None;
        let mut entries = Vec::new();
        let mut in_sha256 = false;
        for line in text.lines() {
            if let Some(entry) = line.strip_prefix(' ') {
                in_sha256
                    .then(|| parse_entry(entry))
                    .transpose()?
                    .into_iter()
                    .for_each(|parsed| entries.push(parsed));
                continue;
            }
            in_sha256 = false;
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            match key {
                "Date" => date = Some(parse_release_date(value.trim())?),
                "Codename" => codename = Some(value.trim().to_owned()),
                "SHA256" => in_sha256 = true,
                _ => {}
            }
        }
        Ok(Self {
            date,
            codename,
            entries,
        })
    }

    /// 查找 `SHA256:` 段中的路径。
    #[must_use]
    pub fn sha256_of(&self, path: &str) -> Option<&ReleaseEntry> {
        self.entries
            .iter()
            .find(|(name, _)| name == path)
            .map(|(_, entry)| entry)
    }

    /// `Date` 早于 `now - 30 天` 时为真。
    #[must_use]
    pub fn is_stale(&self, now: Timestamp) -> bool {
        self.date
            .is_some_and(|date| date.saturating_add(STALE_AFTER) < now)
    }
}

/// 解析 `<sha256> <size> <path>`。
///
/// # Errors
///
/// 字段缺失或格式错误时返回错误。
fn parse_entry(line: &str) -> AssetsResult<(String, ReleaseEntry)> {
    let mut fields = line.split_whitespace();
    let (Some(sha), Some(size), Some(path)) = (fields.next(), fields.next(), fields.next()) else {
        return Err(fail("bad_release", "SHA256 段条目字段不足"));
    };
    if !is_sha256_hex(sha) {
        return Err(fail("bad_release", "SHA256 段哈希不合法"));
    }
    let bytes = size
        .parse()
        .map_err(|_error| fail("bad_release", "SHA256 段大小不合法"))?;
    Ok((
        path.to_owned(),
        ReleaseEntry {
            sha256: sha.to_ascii_lowercase(),
            size: bytes,
        },
    ))
}

/// Debian `Date`（RFC 2822，时区写作 `UTC`）。
///
/// # Errors
///
/// 格式错误时返回错误。
fn parse_release_date(value: &str) -> AssetsResult<Timestamp> {
    let normalized = value
        .strip_suffix(" UTC")
        .map_or_else(|| value.to_owned(), |head| format!("{head} +0000"));
    let parsed = OffsetDateTime::parse(&normalized, &Rfc2822)
        .map_err(|error| AssetsError::new("bad_release", format!("Date 格式错误：{error}")))?;
    to_timestamp(parsed)
}

/// 是否为 64 位十六进制。
#[must_use]
pub fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// `SHA256SUMS` 文件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sha256Sums {
    /// （路径，哈希）；路径去掉 `./` 前缀。
    entries: Vec<(String, String)>,
}

impl Sha256Sums {
    /// 解析 `<sha256>  <path>` 行（`*` 二进制标记可选）。
    ///
    /// # Errors
    ///
    /// 任何一行格式错误时返回错误。
    pub fn parse(text: &str) -> AssetsResult<Self> {
        let mut entries = Vec::new();
        for line in text.lines().filter(|line| !line.trim().is_empty()) {
            let (sha, path) = line
                .split_once(char::is_whitespace)
                .ok_or_else(|| fail("bad_sums", "SHA256SUMS 行缺少路径"))?;
            if !is_sha256_hex(sha) {
                return Err(fail("bad_sums", "SHA256SUMS 哈希不合法"));
            }
            let name = path.trim().trim_start_matches('*');
            let normalized = name.strip_prefix("./").unwrap_or(name);
            entries.push((normalized.to_owned(), sha.to_ascii_lowercase()));
        }
        Ok(Self { entries })
    }

    /// 查找路径（可带或不带 `./`）。
    #[must_use]
    pub fn get(&self, path: &str) -> Option<&str> {
        let normalized = path.strip_prefix("./").unwrap_or(path);
        self.entries
            .iter()
            .find(|(name, _)| name == normalized)
            .map(|(_, sha)| sha.as_str())
    }
}

#[cfg(test)]
mod tests;
