//! `OpenPGP` v4 签名包（RFC 4880 §5.2.3）解析与验证；密码学部分由 `ring` 完成。

use ring::signature::{self as sig, RsaPublicKeyComponents, UnparsedPublicKey};
use sha2::{Sha256, Sha384, Sha512};

use crate::error::{AssetsError, AssetsResult, fail};
use crate::packet::{Reader, packets};
use crate::pubkey::{KeyMaterial, PublicKey};

/// 签名的哈希算法。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HashAlg {
    /// SHA-256（8）。
    Sha256,
    /// SHA-384（9）。
    Sha384,
    /// SHA-512（10）。
    Sha512,
}

impl HashAlg {
    /// 由算法编号解析。
    ///
    /// # Errors
    ///
    /// 不支持的算法（含 SHA-1）返回错误。
    fn from_id(id: u8) -> AssetsResult<Self> {
        match id {
            8 => Ok(Self::Sha256),
            9 => Ok(Self::Sha384),
            10 => Ok(Self::Sha512),
            other => Err(AssetsError::new(
                "unsupported_hash",
                format!("不支持的签名哈希算法 {other}（只接受 SHA-256/384/512）"),
            )),
        }
    }

    /// 计算摘要。
    fn digest(self, parts: &[&[u8]]) -> Vec<u8> {
        match self {
            Self::Sha256 => digest_with::<Sha256>(parts),
            Self::Sha384 => digest_with::<Sha384>(parts),
            Self::Sha512 => digest_with::<Sha512>(parts),
        }
    }

    /// 对应的 ring RSA PKCS#1 v1.5 验证参数。
    fn rsa_params(self) -> &'static sig::RsaParameters {
        match self {
            Self::Sha256 => &sig::RSA_PKCS1_2048_8192_SHA256,
            Self::Sha384 => &sig::RSA_PKCS1_2048_8192_SHA384,
            Self::Sha512 => &sig::RSA_PKCS1_2048_8192_SHA512,
        }
    }
}

/// 以指定摘要算法计算。
fn digest_with<D: sha2::Digest>(parts: &[&[u8]]) -> Vec<u8> {
    let mut hasher = D::new();
    for part in parts {
        hasher.update(part);
    }
    hasher.finalize().to_vec()
}

/// 签名数值。
#[derive(Debug, Clone, PartialEq, Eq)]
enum SigValue {
    /// RSA 签名整数。
    Rsa(Vec<u8>),
    /// Ed25519 `R ‖ S`。
    Ed25519([u8; 64]),
    /// 不支持的公钥算法。
    Unsupported(u8),
}

/// 已解析的 v4 签名。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Signature {
    /// 签名类型（0x00 二进制、0x01 规范文本）。
    pub(crate) sig_type: u8,
    /// 哈希算法。
    hash: HashAlg,
    /// 参与哈希的签名头与 hashed 子包（包体前 `6 + hashed_len` 字节）。
    hashed: Vec<u8>,
    /// 签名者指纹（子包 33）。
    pub(crate) issuer_fingerprint: Option<String>,
    /// 签名者长密钥 ID（子包 16）。
    pub(crate) issuer_key_id: Option<String>,
    /// 签名创建时间（子包 2，Unix 秒）。
    pub(crate) created: Option<u32>,
    /// 签名有效期（子包 3，相对创建时间的秒数）。
    pub(crate) expires_after: Option<u32>,
    /// 摘要前 16 位。
    left16: [u8; 2],
    /// 签名数值。
    value: SigValue,
}

/// 解析二进制报文中全部签名包；非 v4 签名包跳过。
///
/// # Errors
///
/// 报文损坏、哈希算法不支持或没有任何 v4 签名时返回错误。
pub(crate) fn parse_signatures(data: &[u8]) -> AssetsResult<Vec<Signature>> {
    let mut out = Vec::new();
    for packet in packets(data)? {
        if packet.tag == 2 && packet.body.first() == Some(&4) {
            out.push(parse_v4(packet.body)?);
        }
    }
    if out.is_empty() {
        return Err(fail("no_signature", "没有 v4 签名包"));
    }
    Ok(out)
}

/// 解析 v4 签名包体。
///
/// # Errors
///
/// 包体截断或哈希算法不支持时返回错误。
fn parse_v4(body: &[u8]) -> AssetsResult<Signature> {
    let mut reader = Reader::new(body);
    let _version = reader.byte()?;
    let sig_type = reader.byte()?;
    let algorithm = reader.byte()?;
    let hash = HashAlg::from_id(reader.byte()?)?;
    let hashed_len = usize::from(reader.u16()?);
    let hashed_area = reader.take(hashed_len)?;
    let hashed = body
        .get(..reader.position())
        .ok_or_else(|| fail("truncated", "签名包截断"))?
        .to_vec();
    let unhashed_len = usize::from(reader.u16()?);
    let unhashed_area = reader.take(unhashed_len)?;
    let left16_raw = reader.take(2)?;
    let left16 = [
        left16_raw.first().copied().unwrap_or_default(),
        left16_raw.get(1).copied().unwrap_or_default(),
    ];
    let value = parse_value(algorithm, &mut reader)?;
    let mut signature = Signature {
        sig_type,
        hash,
        hashed,
        issuer_fingerprint: None,
        issuer_key_id: None,
        created: None,
        expires_after: None,
        left16,
        value,
    };
    apply_subpackets(&mut signature, hashed_area, true)?;
    apply_subpackets(&mut signature, unhashed_area, false)?;
    Ok(signature)
}

/// 解析签名数值。
///
/// # Errors
///
/// 截断时返回错误。
fn parse_value(algorithm: u8, reader: &mut Reader<'_>) -> AssetsResult<SigValue> {
    match algorithm {
        1 | 3 => Ok(SigValue::Rsa(reader.mpi()?.to_vec())),
        22 => {
            let big_r = left_pad::<32>(reader.mpi()?)?;
            let big_s = left_pad::<32>(reader.mpi()?)?;
            let mut out = [0_u8; 64];
            let (head, tail) = out.split_at_mut(32);
            head.copy_from_slice(&big_r);
            tail.copy_from_slice(&big_s);
            Ok(SigValue::Ed25519(out))
        }
        27 => {
            let raw = reader.take(64)?;
            let out: [u8; 64] = raw
                .try_into()
                .map_err(|_error| fail("truncated", "Ed25519 签名截断"))?;
            Ok(SigValue::Ed25519(out))
        }
        other => Ok(SigValue::Unsupported(other)),
    }
}

/// 左补零到 `N` 字节。
///
/// # Errors
///
/// 输入超过 `N` 字节时返回错误。
fn left_pad<const N: usize>(raw: &[u8]) -> AssetsResult<[u8; N]> {
    let offset = N
        .checked_sub(raw.len())
        .ok_or_else(|| fail("bad_signature", "签名数值过长"))?;
    let mut out = [0_u8; N];
    out.get_mut(offset..)
        .ok_or_else(|| fail("bad_signature", "签名数值过长"))?
        .copy_from_slice(raw);
    Ok(out)
}

/// 解析子包区域并写入签名字段。hashed 区域里的未知关键子包按 RFC 4880 拒绝。
///
/// # Errors
///
/// 子包截断或存在未知关键子包时返回错误。
fn apply_subpackets(signature: &mut Signature, area: &[u8], hashed: bool) -> AssetsResult<()> {
    let mut reader = Reader::new(area);
    while !reader.rest_is_empty() {
        let len = subpacket_len(&mut reader)?;
        let body = reader.take(len)?;
        let (kind_byte, data) = body
            .split_first()
            .ok_or_else(|| fail("bad_signature", "空子包"))?;
        let critical = kind_byte & 0x80 != 0;
        let kind = kind_byte & 0x7F;
        match kind {
            2 if hashed => signature.created = Some(be_u32(data)?),
            3 if hashed => signature.expires_after = Some(be_u32(data)?),
            16 if data.len() == 8 => signature.issuer_key_id = Some(hex::encode_upper(data)),
            33 => {
                if let Some((4, fpr)) = data.split_first() {
                    signature.issuer_fingerprint = Some(hex::encode_upper(fpr));
                }
            }
            _ if critical && hashed => {
                return Err(AssetsError::new(
                    "bad_signature",
                    format!("签名含未知关键子包 {kind}"),
                ));
            }
            _ => {}
        }
    }
    Ok(())
}

/// 子包长度（RFC 4880 §5.2.3.1）。
///
/// # Errors
///
/// 截断时返回错误。
fn subpacket_len(reader: &mut Reader<'_>) -> AssetsResult<usize> {
    let first = reader.byte()?;
    if first < 192 {
        Ok(usize::from(first))
    } else if first < 255 {
        let second = reader.byte()?;
        usize::from(first)
            .checked_sub(192)
            .and_then(|value| value.checked_mul(256))
            .and_then(|value| value.checked_add(usize::from(second)))
            .and_then(|value| value.checked_add(192))
            .ok_or_else(|| fail("bad_signature", "子包长度溢出"))
    } else {
        usize::try_from(reader.u32()?).map_err(|_error| fail("bad_signature", "子包长度溢出"))
    }
}

/// 读取 4 字节大端整数。
///
/// # Errors
///
/// 长度不符时返回错误。
fn be_u32(data: &[u8]) -> AssetsResult<u32> {
    let bytes: [u8; 4] = data
        .try_into()
        .map_err(|_error| fail("bad_signature", "时间子包长度不符"))?;
    Ok(u32::from_be_bytes(bytes))
}

impl Signature {
    /// 以 `key` 验证 `data` 上的签名。
    ///
    /// # Errors
    ///
    /// 算法不匹配、摘要前缀不符或密码学验证失败时返回 `bad_signature`。
    pub(crate) fn verify(&self, key: &PublicKey, data: &[u8]) -> AssetsResult<()> {
        let len = u32::try_from(self.hashed.len())
            .map_err(|_error| fail("bad_signature", "hashed 区域过长"))?;
        let mut trailer = vec![0x04, 0xFF];
        trailer.extend_from_slice(&len.to_be_bytes());
        let digest = self.hash.digest(&[data, &self.hashed, &trailer]);
        if digest.get(..2) != Some(self.left16.as_slice()) {
            return Err(fail(
                "bad_signature",
                "摘要前缀不符：数据已被篡改或签名不属于该数据",
            ));
        }
        let outcome = match (&key.material, &self.value) {
            (KeyMaterial::Rsa { modulus, exponent }, SigValue::Rsa(value)) => {
                let message = [data, &self.hashed, &trailer].concat();
                let mut padded = vec![0_u8; modulus.len().saturating_sub(value.len())];
                padded.extend_from_slice(value);
                RsaPublicKeyComponents {
                    n: modulus.as_slice(),
                    e: exponent.as_slice(),
                }
                .verify(self.hash.rsa_params(), &message, &padded)
            }
            (KeyMaterial::Ed25519(point), SigValue::Ed25519(value)) => {
                UnparsedPublicKey::new(&sig::ED25519, point).verify(&digest, value)
            }
            (
                KeyMaterial::Rsa { .. } | KeyMaterial::Ed25519(_) | KeyMaterial::Unsupported(_),
                _,
            ) => {
                return Err(fail("bad_signature", "签名算法与公钥算法不匹配或不受支持"));
            }
        };
        outcome.map_err(|_error| fail("bad_signature", "签名密码学验证失败"))
    }

    /// 签名在 `now`（Unix 秒）是否已过期。
    pub(crate) fn expired_at(&self, now: u64) -> bool {
        match (self.created, self.expires_after) {
            (Some(created), Some(after)) if after > 0 => {
                u64::from(created).saturating_add(u64::from(after)) <= now
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests;
