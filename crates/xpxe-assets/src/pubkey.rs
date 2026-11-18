//! `OpenPGP` v4 公钥与子钥解析（RFC 4880 §5.5，RFC 9580 Ed25519）。

use sha1::{Digest as _, Sha1};

use crate::armor::decode_blocks;
use crate::error::{AssetsError, AssetsResult, fail};
use crate::packet::{Reader, packets};

/// 公钥装甲标签。
const KEY_LABEL: &str = "PGP PUBLIC KEY BLOCK";
/// Ed25519 曲线 OID（1.3.6.1.4.1.11591.15.1）。
const ED25519_OID: [u8; 9] = [0x2B, 0x06, 0x01, 0x04, 0x01, 0xDA, 0x47, 0x0F, 0x01];

/// 已解析的公钥信息（主钥指纹）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicKeyInfo {
    /// `OpenPGP` v4 指纹（40 位大写十六进制）。
    fingerprint: String,
}

impl PublicKeyInfo {
    /// 指纹。
    #[must_use]
    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }
}

/// 解析恰好一个 ASCII-armored 公钥块并计算主钥指纹；块外不得有其他内容。
///
/// # Errors
///
/// 装甲无法解码、块外有内容、报文损坏或找不到 v4 公钥包时返回错误。
pub fn parse_public_key(armored: &str) -> AssetsResult<PublicKeyInfo> {
    let trimmed = armored.trim();
    let strict = trimmed.starts_with("-----BEGIN PGP PUBLIC KEY BLOCK-----")
        && trimmed.ends_with("-----END PGP PUBLIC KEY BLOCK-----");
    if !strict {
        return Err(fail("bad_armor", "缺少 PGP PUBLIC KEY 装甲头或尾"));
    }
    let blocks = decode_blocks(trimmed, KEY_LABEL)?;
    if blocks.len() != 1 {
        return Err(fail("bad_armor", "只允许一个公钥装甲块"));
    }
    let ring = Keyring::from_binary(blocks.first().map_or(&[][..], Vec::as_slice))?;
    let primary = ring
        .keys
        .iter()
        .find(|key| key.primary_fingerprint == key.fingerprint)
        .ok_or_else(|| fail("no_public_key", "装甲中没有 v4 公钥包"))?;
    Ok(PublicKeyInfo {
        fingerprint: primary.fingerprint.clone(),
    })
}

/// 公钥算法材料。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum KeyMaterial {
    /// RSA（算法 1/3）。
    Rsa {
        /// 模数。
        modulus: Vec<u8>,
        /// 公开指数。
        exponent: Vec<u8>,
    },
    /// Ed25519（算法 22 旧式 `EdDSA` 或 27 原生）。
    Ed25519([u8; 32]),
    /// 不支持的算法（只记录编号，不可用于验证）。
    Unsupported(u8),
}

/// 一把 v4 公钥（主钥或子钥）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PublicKey {
    /// 本钥指纹。
    pub(crate) fingerprint: String,
    /// 所属主钥指纹（主钥时等于自身）。
    pub(crate) primary_fingerprint: String,
    /// 算法材料。
    pub(crate) material: KeyMaterial,
}

impl PublicKey {
    /// 长密钥 ID（指纹后 16 位）。
    pub(crate) fn key_id(&self) -> &str {
        self.fingerprint.get(24..).unwrap_or_default()
    }
}

/// 公钥环。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Keyring {
    /// 主钥与子钥。
    pub(crate) keys: Vec<PublicKey>,
}

impl Keyring {
    /// 解析文本中全部公钥装甲块。
    ///
    /// # Errors
    ///
    /// 装甲或报文损坏时返回错误。
    pub(crate) fn from_armored(armored: &str) -> AssetsResult<Self> {
        let mut ring = Self::default();
        for block in decode_blocks(armored, KEY_LABEL)? {
            ring.keys.extend(Self::from_binary(&block)?.keys);
        }
        Ok(ring)
    }

    /// 解析二进制报文序列：tag 6 为主钥，tag 14 为其后的子钥；非 v4 钥跳过。
    ///
    /// # Errors
    ///
    /// 报文损坏时返回错误。
    pub(crate) fn from_binary(data: &[u8]) -> AssetsResult<Self> {
        let mut keys = Vec::new();
        let mut primary: Option<String> = None;
        for packet in packets(data)? {
            if packet.tag != 6 && packet.tag != 14 {
                continue;
            }
            let parsed = parse_v4_key(packet.body)?;
            if packet.tag == 6 {
                primary = parsed.as_ref().map(|(fingerprint, _)| fingerprint.clone());
            }
            let Some((fingerprint, material)) = parsed else {
                continue;
            };
            if let Some(owner) = primary.as_ref() {
                keys.push(PublicKey {
                    fingerprint,
                    primary_fingerprint: owner.clone(),
                    material,
                });
            }
        }
        if keys.is_empty() {
            return Err(fail("no_public_key", "装甲中没有 v4 公钥包"));
        }
        Ok(Self { keys })
    }

    /// 按指纹或长密钥 ID 查找。
    pub(crate) fn find(
        &self,
        fingerprint: Option<&str>,
        key_id: Option<&str>,
    ) -> Option<&PublicKey> {
        self.keys.iter().find(|key| {
            fingerprint.is_some_and(|value| key.fingerprint == value)
                || key_id.is_some_and(|value| key.key_id() == value)
        })
    }
}

/// 解析 v4 公钥包体：返回指纹与算法材料；非 v4 返回 `None`。
///
/// # Errors
///
/// 包体截断时返回错误。
fn parse_v4_key(body: &[u8]) -> AssetsResult<Option<(String, KeyMaterial)>> {
    let mut reader = Reader::new(body);
    if reader.byte()? != 4 {
        return Ok(None);
    }
    let _created = reader.u32()?;
    let algorithm = reader.byte()?;
    let material = match algorithm {
        1 | 3 => KeyMaterial::Rsa {
            modulus: reader.mpi()?.to_vec(),
            exponent: reader.mpi()?.to_vec(),
        },
        22 => legacy_eddsa(&mut reader)?,
        27 => KeyMaterial::Ed25519(to_array(reader.take(32)?)?),
        other => KeyMaterial::Unsupported(other),
    };
    let len = u16::try_from(body.len()).map_err(|_error| fail("bad_key", "公钥包过长"))?;
    let mut hasher = Sha1::new();
    hasher.update([0x99]);
    hasher.update(len.to_be_bytes());
    hasher.update(body);
    Ok(Some((hex::encode_upper(hasher.finalize()), material)))
}

/// 旧式 `EdDSA`（算法 22）：OID + 0x40 前缀的压缩点。
///
/// # Errors
///
/// 包体截断时返回错误。
fn legacy_eddsa(reader: &mut Reader<'_>) -> AssetsResult<KeyMaterial> {
    let oid_len = usize::from(reader.byte()?);
    let oid = reader.take(oid_len)?;
    let point = reader.mpi()?;
    if oid != ED25519_OID {
        return Ok(KeyMaterial::Unsupported(22));
    }
    match point.split_first() {
        Some((0x40, raw)) => Ok(KeyMaterial::Ed25519(to_array(raw)?)),
        Some(_) | None => Err(fail("bad_key", "Ed25519 公钥点格式不符")),
    }
}

/// 切片转 32 字节数组。
///
/// # Errors
///
/// 长度不为 32 时返回错误。
pub(crate) fn to_array(raw: &[u8]) -> AssetsResult<[u8; 32]> {
    raw.try_into()
        .map_err(|_error| AssetsError::new("bad_key", format!("期望 32 字节，实际 {}", raw.len())))
}

#[cfg(test)]
mod tests;
