//! ASCII-armored `OpenPGP` 公钥解析（v4 指纹）。

use sha1::{Digest as _, Sha1};
use std::fmt::{Display, Formatter};

/// 资产校验错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetsError {
    /// 稳定错误码。
    code: &'static str,
    /// 说明。
    message: String,
}

impl AssetsError {
    /// 构造错误。
    #[must_use]
    pub const fn new(code: &'static str, message: String) -> Self {
        Self { code, message }
    }

    /// 错误码。
    #[must_use]
    pub const fn code(&self) -> &'static str {
        self.code
    }
}

impl Display for AssetsError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for AssetsError {}

/// 已解析的公钥信息。
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

/// 解析 ASCII-armored 公钥并计算主密钥指纹。
///
/// # Errors
///
/// 装甲无法解码、报文损坏或找不到 v4 公钥包时返回错误。
pub fn parse_public_key(armored: &str) -> Result<PublicKeyInfo, AssetsError> {
    let body = decode_armor(armored)?;
    let fingerprint = first_v4_fingerprint(&body)?;
    Ok(PublicKeyInfo { fingerprint })
}

/// 解码装甲块为二进制。
///
/// # Errors
///
/// 装甲外形或 Base64 不合法时返回错误。
fn decode_armor(raw: &str) -> Result<Vec<u8>, AssetsError> {
    const BEGIN: &str = "-----BEGIN PGP PUBLIC KEY BLOCK-----";
    const END: &str = "-----END PGP PUBLIC KEY BLOCK-----";
    let trimmed = raw.trim();
    let rest = trimmed
        .strip_prefix(BEGIN)
        .and_then(|value| value.strip_suffix(END))
        .ok_or_else(|| {
            AssetsError::new("bad_armor", "缺少 PGP PUBLIC KEY 装甲头或尾".to_owned())
        })?;
    let payload = strip_armor_headers(rest);
    let (b64, _crc) = split_crc(payload);
    let compact: String = b64.chars().filter(|ch| !ch.is_whitespace()).collect();
    base64::Engine::decode(
        &base64::engine::general_purpose::STANDARD,
        compact.as_bytes(),
    )
    .map_err(|error| AssetsError::new("bad_base64", format!("Base64 解码失败：{error}")))
}

/// 去掉装甲头（至首个空行）。
fn strip_armor_headers(payload: &str) -> &str {
    let text = payload.trim_start_matches(['\r', '\n']);
    if let Some((headers, body)) = text.split_once("\n\n")
        && headers.lines().all(is_armor_header_line)
    {
        return body;
    }
    if let Some((headers, body)) = text.split_once("\r\n\r\n")
        && headers.lines().all(is_armor_header_line)
    {
        return body;
    }
    text
}

/// 是否为 `Key: Value` 形式的装甲头。
fn is_armor_header_line(line: &str) -> bool {
    line.is_empty() || line.contains(':')
}

/// 拆出可选的 CRC 行（以 `=` 开头）。
fn split_crc(payload: &str) -> (&str, Option<&str>) {
    payload.rfind("\n=").map_or((payload, None), |index| {
        payload
            .split_at_checked(index)
            .map_or((payload, None), |(body, crc)| (body, Some(crc.trim())))
    })
}

/// 遍历报文，返回第一把 v4 主公钥的指纹。
///
/// # Errors
///
/// 报文截断或没有 v4 公钥包时返回错误。
fn first_v4_fingerprint(data: &[u8]) -> Result<String, AssetsError> {
    let mut offset = 0_usize;
    while offset < data.len() {
        let parsed = parse_packet(data, offset)?;
        if parsed.tag == 6
            && let Some(fingerprint) = v4_fingerprint(parsed.body)
        {
            return Ok(fingerprint);
        }
        offset = parsed.next;
    }
    Err(AssetsError::new(
        "no_public_key",
        "装甲中没有 v4 公钥包".to_owned(),
    ))
}

/// 一个报文的解析结果。
struct PacketView<'data> {
    /// 包类型。
    tag: u8,
    /// 包体。
    body: &'data [u8],
    /// 下一包起始偏移。
    next: usize,
}

/// 解析一个报文头与包体。
///
/// # Errors
///
/// 长度字段截断时返回错误。
fn parse_packet(data: &[u8], offset: usize) -> Result<PacketView<'_>, AssetsError> {
    let first = *data
        .get(offset)
        .ok_or_else(|| AssetsError::new("truncated", "报文截断".to_owned()))?;
    if (first & 0x80) == 0 {
        return Err(AssetsError::new("bad_packet", "非法报文头".to_owned()));
    }
    if (first & 0x40) == 0 {
        parse_old_packet(data, offset, first)
    } else {
        parse_new_packet(data, offset, first)
    }
}

/// 旧格式报文。
///
/// # Errors
///
/// 长度截断时返回错误。
fn parse_old_packet(data: &[u8], offset: usize, first: u8) -> Result<PacketView<'_>, AssetsError> {
    let tag = (first & 0x3c).checked_shr(2).unwrap_or(0);
    let length_type = first & 0x03;
    let start = offset
        .checked_add(1)
        .ok_or_else(|| AssetsError::new("truncated", "报文偏移溢出".to_owned()))?;
    let (len, body_at) = old_length(data, start, length_type)?;
    slice_packet(data, tag, body_at, len)
}

/// 新格式报文。
///
/// # Errors
///
/// 长度截断时返回错误。
fn parse_new_packet(data: &[u8], offset: usize, first: u8) -> Result<PacketView<'_>, AssetsError> {
    let tag = first & 0x3f;
    let start = offset
        .checked_add(1)
        .ok_or_else(|| AssetsError::new("truncated", "报文偏移溢出".to_owned()))?;
    let (len, body_at) = new_length(data, start)?;
    slice_packet(data, tag, body_at, len)
}

/// 按长度切出包体。
///
/// # Errors
///
/// 超出缓冲区时返回错误。
fn slice_packet(
    data: &[u8],
    tag: u8,
    body_at: usize,
    len: usize,
) -> Result<PacketView<'_>, AssetsError> {
    let end = body_at
        .checked_add(len)
        .ok_or_else(|| AssetsError::new("truncated", "包长度溢出".to_owned()))?;
    let body = data
        .get(body_at..end)
        .ok_or_else(|| AssetsError::new("truncated", "包体超出缓冲区".to_owned()))?;
    Ok(PacketView {
        tag,
        body,
        next: end,
    })
}

/// 旧格式长度。
///
/// # Errors
///
/// 截断或不定长时返回错误。
fn old_length(data: &[u8], start: usize, length_type: u8) -> Result<(usize, usize), AssetsError> {
    match length_type {
        0 => read_len(data, start, 1),
        1 => read_len(data, start, 2),
        2 => read_len(data, start, 4),
        _ => Err(AssetsError::new(
            "indeterminate",
            "不支持不定长报文".to_owned(),
        )),
    }
}

/// 新格式长度。
///
/// # Errors
///
/// 截断或部分长度时返回错误。
fn new_length(data: &[u8], start: usize) -> Result<(usize, usize), AssetsError> {
    let first = *data
        .get(start)
        .ok_or_else(|| AssetsError::new("truncated", "缺少长度字节".to_owned()))?;
    let next = start
        .checked_add(1)
        .ok_or_else(|| AssetsError::new("truncated", "长度偏移溢出".to_owned()))?;
    if first < 192 {
        Ok((usize::from(first), next))
    } else if first < 224 {
        let second = *data
            .get(next)
            .ok_or_else(|| AssetsError::new("truncated", "缺少第二长度字节".to_owned()))?;
        let hi = usize::from(first)
            .checked_sub(192)
            .and_then(|value| value.checked_mul(256))
            .and_then(|value| value.checked_add(usize::from(second)))
            .and_then(|value| value.checked_add(192))
            .ok_or_else(|| AssetsError::new("truncated", "长度计算溢出".to_owned()))?;
        let body_at = next
            .checked_add(1)
            .ok_or_else(|| AssetsError::new("truncated", "长度偏移溢出".to_owned()))?;
        Ok((hi, body_at))
    } else if first == 255 {
        read_len(data, next, 4)
    } else {
        Err(AssetsError::new(
            "partial_length",
            "不支持部分长度报文".to_owned(),
        ))
    }
}

/// 以大端读取 `width` 字节长度。
///
/// # Errors
///
/// 截断时返回错误。
fn read_len(data: &[u8], start: usize, width: usize) -> Result<(usize, usize), AssetsError> {
    let end = start
        .checked_add(width)
        .ok_or_else(|| AssetsError::new("truncated", "长度字段溢出".to_owned()))?;
    let bytes = data
        .get(start..end)
        .ok_or_else(|| AssetsError::new("truncated", "长度字段截断".to_owned()))?;
    let mut value = 0_usize;
    for byte in bytes {
        value = value
            .checked_mul(256)
            .and_then(|current| current.checked_add(usize::from(*byte)))
            .ok_or_else(|| AssetsError::new("truncated", "长度溢出".to_owned()))?;
    }
    Ok((value, end))
}

/// 计算 v4 公钥指纹；非 v4 返回 `None`。
fn v4_fingerprint(body: &[u8]) -> Option<String> {
    let version = *body.first()?;
    if version != 4 {
        return None;
    }
    let len = u16::try_from(body.len()).ok()?;
    let mut hasher = Sha1::new();
    hasher.update([0x99]);
    hasher.update(len.to_be_bytes());
    hasher.update(body);
    Some(hex::encode_upper(hasher.finalize()))
}

#[cfg(test)]
mod tests;
