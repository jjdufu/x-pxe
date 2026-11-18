//! 信任根：内置 Debian 公钥 + 指纹白名单（含过期时间），以及内联/分离签名验证。

use serde::Serialize;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use xpxe_domain::Timestamp;

use crate::armor::{decode_blocks, split_cleartext};
use crate::error::{AssetsError, AssetsResult, fail};
use crate::pubkey::Keyring;
use crate::signature::{Signature, parse_signatures};

/// 内置 Debian 归档签名公钥（从 `debian-archive-keyring` 导出）。
const DEBIAN_ARCHIVE_KEYRING: &str = include_str!("../trust/debian-archive-keyring.asc");
/// 内置 Debian CD 签名公钥（`firmware` 的 `SHA256SUMS.sign`）。
const DEBIAN_CD_KEY: &str = include_str!("../trust/debian-cd-signing-key.asc");
/// 指纹白名单。
const DEBIAN_FINGERPRINTS: &str = include_str!("../trust/fingerprints.txt");

/// 白名单中的一条指纹。
#[derive(Debug, Clone, PartialEq, Eq)]
struct Pin {
    /// 40 位大写十六进制指纹。
    fingerprint: String,
    /// 过期时间；`None` 表示不过期。
    expires: Option<Timestamp>,
    /// 说明。
    label: String,
}

/// 验证通过的签名者。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Signer {
    /// 签名所用密钥（可能是子钥）的指纹。
    pub fingerprint: String,
    /// 所属主钥指纹。
    pub primary_fingerprint: String,
    /// 白名单说明。
    pub label: String,
    /// 签名创建时间（Unix 秒）。
    pub created: Option<u64>,
}

/// 验证通过的内联签名消息。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClearSigned {
    /// 去除破折号转义后的原文。
    pub text: String,
    /// 签名者。
    pub signer: Signer,
}

/// 信任根。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustStore {
    /// 公钥环。
    keyring: Keyring,
    /// 指纹白名单。
    pins: Vec<Pin>,
}

impl TrustStore {
    /// 构建期编入的 Debian 信任根。
    ///
    /// # Errors
    ///
    /// 内置文件损坏时返回错误（由单元测试保证不会发生）。
    pub fn debian() -> AssetsResult<Self> {
        Self::from_parts(
            &[DEBIAN_ARCHIVE_KEYRING, DEBIAN_CD_KEY],
            DEBIAN_FINGERPRINTS,
        )
    }

    /// 由公钥装甲与白名单文本构建（测试与内网自建信任根）。
    ///
    /// # Errors
    ///
    /// 装甲损坏、白名单格式错误，或白名单指纹不在公钥环中时返回错误。
    pub fn from_parts(armored: &[&str], fingerprints: &str) -> AssetsResult<Self> {
        let mut keyring = Keyring::default();
        for text in armored {
            keyring.keys.extend(Keyring::from_armored(text)?.keys);
        }
        let pins = parse_pins(fingerprints)?;
        for pin in &pins {
            if keyring.find(Some(&pin.fingerprint), None).is_none() {
                return Err(AssetsError::new(
                    "bad_trust",
                    format!("白名单指纹 {} 不在公钥环中", pin.fingerprint),
                ));
            }
        }
        Ok(Self { keyring, pins })
    }

    /// 白名单中的全部指纹。
    pub fn fingerprints(&self) -> impl Iterator<Item = &str> {
        self.pins.iter().map(|pin| pin.fingerprint.as_str())
    }

    /// 验证内联签名消息（如 `InRelease`）：至少一个签名来自白名单内未过期的密钥且验证通过。
    ///
    /// # Errors
    ///
    /// 结构损坏或没有可信签名时返回错误；错误码见 [`TrustStore::verify_detached`]。
    pub fn verify_cleartext(&self, raw: &str, now: Timestamp) -> AssetsResult<ClearSigned> {
        let parts = split_cleartext(raw)?;
        let signatures = parse_signatures(&parts.signature)?;
        let signer = self.check(&signatures, &parts.canonical, &parts.canonical, now)?;
        Ok(ClearSigned {
            text: parts.text,
            signer,
        })
    }

    /// 验证分离签名（二进制或 `PGP SIGNATURE` 装甲）。
    ///
    /// # Errors
    ///
    /// - `bad_signature`：来自可信密钥的签名验证失败（数据被篡改）；
    /// - `key_expired`：签名密钥在白名单中已过期，或签名本身过期；
    /// - `untrusted_signer`：签名密钥不在白名单中；
    /// - 其他：签名结构损坏。
    pub fn verify_detached(
        &self,
        data: &[u8],
        signature: &[u8],
        now: Timestamp,
    ) -> AssetsResult<Signer> {
        let binary = match std::str::from_utf8(signature) {
            Ok(text) if text.contains("-----BEGIN PGP SIGNATURE-----") => {
                decode_blocks(text, "PGP SIGNATURE")?
                    .into_iter()
                    .next()
                    .ok_or_else(|| fail("no_signature", "签名块为空"))?
            }
            Ok(_) | Err(_) => signature.to_vec(),
        };
        let signatures = parse_signatures(&binary)?;
        let text_form = canonical_text(data);
        self.check(&signatures, data, &text_form, now)
    }

    /// 逐个签名检查：返回第一个可信且验证通过的签名者，否则按严重程度报错。
    ///
    /// # Errors
    ///
    /// 没有可信签名时返回错误。
    fn check(
        &self,
        signatures: &[Signature],
        binary: &[u8],
        text: &[u8],
        now: Timestamp,
    ) -> AssetsResult<Signer> {
        let mut worst: Option<AssetsError> = None;
        for signature in signatures {
            match self.check_one(signature, binary, text, now) {
                Ok(signer) => return Ok(signer),
                Err(error) if worst.as_ref().is_none_or(|old| rank(&error) > rank(old)) => {
                    worst = Some(error);
                }
                Err(_) => {}
            }
        }
        Err(worst.unwrap_or_else(|| fail("no_signature", "没有签名")))
    }

    /// 检查单个签名。
    ///
    /// # Errors
    ///
    /// 不可信、过期或验证失败时返回错误。
    fn check_one(
        &self,
        signature: &Signature,
        binary: &[u8],
        text: &[u8],
        now: Timestamp,
    ) -> AssetsResult<Signer> {
        let key = self
            .keyring
            .find(
                signature.issuer_fingerprint.as_deref(),
                signature.issuer_key_id.as_deref(),
            )
            .ok_or_else(|| fail("untrusted_signer", "签名密钥不在信任根中"))?;
        let pin = self
            .pins
            .iter()
            .find(|pin| pin.fingerprint == key.fingerprint)
            .ok_or_else(|| {
                AssetsError::new(
                    "untrusted_signer",
                    format!("签名密钥 {} 不在指纹白名单中", key.fingerprint),
                )
            })?;
        if pin.expires.is_some_and(|expires| expires <= now) {
            return Err(AssetsError::new(
                "key_expired",
                format!("签名密钥 {}（{}）已过期", key.fingerprint, pin.label),
            ));
        }
        if signature.expired_at(now.unix_seconds()) {
            return Err(fail("key_expired", "签名本身已过期"));
        }
        let data = match signature.sig_type {
            0x00 => binary,
            0x01 => text,
            other => {
                return Err(AssetsError::new(
                    "bad_signature",
                    format!("不支持的签名类型 0x{other:02x}"),
                ));
            }
        };
        signature.verify(key, data)?;
        Ok(Signer {
            fingerprint: key.fingerprint.clone(),
            primary_fingerprint: key.primary_fingerprint.clone(),
            label: pin.label.clone(),
            created: signature.created.map(u64::from),
        })
    }
}

/// 错误严重程度：验证失败 > 过期 > 不可信 > 其他。
const fn rank(error: &AssetsError) -> u8 {
    match error.code().as_bytes() {
        b"bad_signature" => 3,
        b"key_expired" => 2,
        b"untrusted_signer" => 1,
        _ => 0,
    }
}

/// 规范文本形式（签名类型 0x01）：行尾统一为 `\r\n`。
fn canonical_text(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut previous = 0_u8;
    for byte in data {
        if *byte == b'\n' && previous != b'\r' {
            out.push(b'\r');
        }
        out.push(*byte);
        previous = *byte;
    }
    out
}

/// 解析白名单：`<40 位指纹> <RFC 3339 过期时间或 -> <说明>`，`#` 开头为注释。
///
/// # Errors
///
/// 指纹或时间格式错误时返回错误。
fn parse_pins(text: &str) -> AssetsResult<Vec<Pin>> {
    let mut pins = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let mut fields = trimmed.splitn(3, char::is_whitespace);
        let fingerprint = fields.next().unwrap_or_default().to_ascii_uppercase();
        let expiry = fields.next().unwrap_or_default();
        let label = fields.next().unwrap_or_default().trim().to_owned();
        let line_no = index.saturating_add(1);
        if fingerprint.len() != 40 || !fingerprint.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(AssetsError::new(
                "bad_trust",
                format!("白名单第 {line_no} 行指纹不合法"),
            ));
        }
        let expires = if expiry == "-" {
            None
        } else {
            Some(parse_rfc3339(expiry).map_err(|_error| {
                AssetsError::new("bad_trust", format!("白名单第 {line_no} 行过期时间不合法"))
            })?)
        };
        pins.push(Pin {
            fingerprint,
            expires,
            label,
        });
    }
    Ok(pins)
}

/// 解析 RFC 3339 时间为 [`Timestamp`]。
///
/// # Errors
///
/// 格式错误或早于 1970 年时返回错误。
pub(crate) fn parse_rfc3339(text: &str) -> AssetsResult<Timestamp> {
    let parsed = OffsetDateTime::parse(text, &Rfc3339)
        .map_err(|error| AssetsError::new("bad_time", format!("时间格式错误：{error}")))?;
    to_timestamp(parsed)
}

/// `OffsetDateTime` 转 [`Timestamp`]。
///
/// # Errors
///
/// 早于 1970 年时返回错误。
pub(crate) fn to_timestamp(value: OffsetDateTime) -> AssetsResult<Timestamp> {
    u64::try_from(value.unix_timestamp())
        .map(Timestamp::from_unix_seconds)
        .map_err(|_error| fail("bad_time", "时间早于 1970 年"))
}

#[cfg(test)]
mod tests;
