//! `OpenPGP` ASCII 装甲（RFC 4880 §6）与内联签名消息（§7）。

use crate::error::{AssetsError, AssetsResult, fail};

/// 装甲尾行前缀。
const END_PREFIX: &str = "-----END ";
/// 内联签名消息头。
const SIGNED_BEGIN: &str = "-----BEGIN PGP SIGNED MESSAGE-----";
/// 签名块头。
const SIGNATURE_BEGIN: &str = "-----BEGIN PGP SIGNATURE-----";

/// 解码文本中全部 `label` 类型的装甲块（如 `PGP PUBLIC KEY BLOCK`）。
///
/// # Errors
///
/// 没有找到装甲块、块不完整、Base64 或 CRC24 不合法时返回错误。
pub(crate) fn decode_blocks(raw: &str, label: &str) -> AssetsResult<Vec<Vec<u8>>> {
    let begin = format!("-----BEGIN {label}-----");
    let end = format!("{END_PREFIX}{label}-----");
    let mut blocks = Vec::new();
    let mut current: Option<Vec<&str>> = None;
    for line in raw
        .lines()
        .map(|line| line.trim_end_matches(['\r', ' ', '\t']))
    {
        if line == begin {
            current = Some(Vec::new());
        } else if line == end {
            let body = current
                .take()
                .ok_or_else(|| fail("bad_armor", "装甲尾行之前没有头行"))?;
            blocks.push(decode_body(&body)?);
        } else if let Some(body) = current.as_mut() {
            body.push(line);
        } else {
            // 装甲块之外的内容忽略
        }
    }
    if current.is_some() {
        return Err(fail("bad_armor", "装甲块缺少尾行"));
    }
    if blocks.is_empty() {
        return Err(AssetsError::new(
            "bad_armor",
            format!("缺少 {label} 装甲块"),
        ));
    }
    Ok(blocks)
}

/// 解码装甲体：跳过头部，拼接 Base64，校验可选的 CRC24。
///
/// # Errors
///
/// Base64 或 CRC24 不合法时返回错误。
fn decode_body(lines: &[&str]) -> AssetsResult<Vec<u8>> {
    let start = header_end(lines);
    let mut encoded = String::new();
    let mut checksum: Option<&str> = None;
    for line in lines.get(start..).unwrap_or_default() {
        if let Some(crc) = line.strip_prefix('=')
            && crc.len() == 4
        {
            checksum = Some(crc);
            break;
        }
        encoded.push_str(line.trim());
    }
    let data = b64(&encoded)?;
    if let Some(crc) = checksum {
        let expected = b64(crc)?;
        let actual = crc24(&data);
        if expected.as_slice() != actual.get(1..).unwrap_or_default() {
            return Err(fail("bad_armor", "装甲 CRC24 校验失败"));
        }
    }
    Ok(data)
}

/// 装甲头结束位置：存在空行且其前各行都是 `Key: Value` 时，从空行之后开始。
fn header_end(lines: &[&str]) -> usize {
    lines
        .iter()
        .position(|line| line.is_empty())
        .filter(|blank| {
            lines
                .get(..*blank)
                .unwrap_or_default()
                .iter()
                .all(|line| line.contains(": "))
        })
        .map_or(0, |blank| blank.saturating_add(1))
}

/// 标准 Base64 解码。
///
/// # Errors
///
/// 输入不是合法 Base64 时返回错误。
fn b64(text: &str) -> AssetsResult<Vec<u8>> {
    base64::Engine::decode(&base64::engine::general_purpose::STANDARD, text.as_bytes())
        .map_err(|error| AssetsError::new("bad_base64", format!("Base64 解码失败：{error}")))
}

/// RFC 4880 §6.1 CRC24，返回 4 字节大端（首字节恒为 0）。
fn crc24(data: &[u8]) -> [u8; 4] {
    const INIT: u32 = 0x00B7_04CE;
    const POLY: u32 = 0x0186_4CFB;
    let mut crc = INIT;
    for byte in data {
        crc ^= u32::from(*byte).wrapping_shl(16);
        for _ in 0_u8..8 {
            crc = crc.wrapping_shl(1);
            if crc & 0x0100_0000 != 0 {
                crc ^= POLY;
            }
        }
    }
    (crc & 0x00FF_FFFF).to_be_bytes()
}

/// 拆开的内联签名消息。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ClearText {
    /// 原文（已去除破折号转义，`\n` 换行，末尾带换行）。
    pub(crate) text: String,
    /// 规范化后的签名数据（去行尾空白、`\r\n` 连接、末行无换行）。
    pub(crate) canonical: Vec<u8>,
    /// 签名块（二进制报文）。
    pub(crate) signature: Vec<u8>,
}

/// 拆分内联签名消息。
///
/// # Errors
///
/// 结构不完整或签名装甲损坏时返回错误。
pub(crate) fn split_cleartext(raw: &str) -> AssetsResult<ClearText> {
    let lines: Vec<&str> = raw
        .lines()
        .map(|line| line.trim_end_matches('\r'))
        .collect();
    let head = lines
        .iter()
        .position(|line| line.trim_end() == SIGNED_BEGIN)
        .ok_or_else(|| fail("not_clearsigned", "缺少 PGP SIGNED MESSAGE 头"))?;
    let rest = lines.get(head.saturating_add(1)..).unwrap_or_default();
    let blank = rest
        .iter()
        .position(|line| line.trim_end().is_empty())
        .ok_or_else(|| fail("not_clearsigned", "签名消息头后缺少空行"))?;
    let body_and_sig = rest.get(blank.saturating_add(1)..).unwrap_or_default();
    let sig_at = body_and_sig
        .iter()
        .position(|line| line.trim_end() == SIGNATURE_BEGIN)
        .ok_or_else(|| fail("not_clearsigned", "缺少 PGP SIGNATURE 块"))?;
    let (body, sig) = body_and_sig.split_at(sig_at);
    let mut text = String::new();
    let mut canonical = Vec::new();
    for (index, line) in body.iter().enumerate() {
        let unescaped = line.strip_prefix("- ").unwrap_or(line);
        text.push_str(unescaped);
        text.push('\n');
        if index > 0 {
            canonical.extend_from_slice(b"\r\n");
        }
        canonical.extend_from_slice(unescaped.trim_end_matches([' ', '\t']).as_bytes());
    }
    let first = decode_blocks(&sig.join("\n"), "PGP SIGNATURE")?
        .into_iter()
        .next()
        .ok_or_else(|| fail("not_clearsigned", "签名块为空"))?;
    Ok(ClearText {
        text,
        canonical,
        signature: first,
    })
}

#[cfg(test)]
mod tests;
