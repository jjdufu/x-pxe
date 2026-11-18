//! 极简 DER 解析：只从 X.509 证书中取主题、颁发者与有效期，供 `probe` 给管理员核对。
//!
//! 不做任何信任判断（信任只看指纹），解析失败时这些字段为空，不影响探测。

use crate::{RedfishError, RedfishResult};

/// 证书摘要。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CertSummary {
    /// 主题，例如 `CN=10.77.0.1, O=lab`。
    pub subject: String,
    /// 颁发者。
    pub issuer: String,
    /// 生效时间（`YYYY-MM-DDTHH:MM:SSZ`）。
    pub not_before: String,
    /// 过期时间（`YYYY-MM-DDTHH:MM:SSZ`）。
    pub not_after: String,
}

/// 一个 DER TLV。
#[derive(Debug, Clone, Copy)]
struct Tlv<'der> {
    /// 标签字节。
    tag: u8,
    /// 内容。
    value: &'der [u8],
}

/// 结构错误。
fn bad() -> RedfishError {
    RedfishError::new("bad_certificate", "证书 DER 结构不合法".to_owned())
}

/// 读一个 TLV，返回它与剩余字节。
///
/// # Errors
///
/// 长度越界或使用不支持的长度编码时返回错误。
fn next_tlv(input: &[u8]) -> RedfishResult<(Tlv<'_>, &[u8])> {
    let (&tag, after_tag) = input.split_first().ok_or_else(bad)?;
    let (&first, mut rest) = after_tag.split_first().ok_or_else(bad)?;
    let len = if first < 0x80 {
        usize::from(first)
    } else {
        let count = usize::from(first & 0x7F);
        if count == 0 || count > 4 {
            return Err(bad());
        }
        let (digits, after) = rest.split_at_checked(count).ok_or_else(bad)?;
        rest = after;
        digits.iter().try_fold(0_usize, |acc, digit| {
            acc.checked_mul(256)
                .and_then(|shifted| shifted.checked_add(usize::from(*digit)))
                .ok_or_else(bad)
        })?
    };
    let (value, after) = rest.split_at_checked(len).ok_or_else(bad)?;
    Ok((Tlv { tag, value }, after))
}

/// 把一个构造型 TLV 的内容拆成子项。
///
/// # Errors
///
/// 子项结构不合法时返回错误。
fn children(mut input: &[u8]) -> RedfishResult<Vec<Tlv<'_>>> {
    let mut out = Vec::new();
    while !input.is_empty() {
        let (item, rest) = next_tlv(input)?;
        out.push(item);
        input = rest;
    }
    Ok(out)
}

/// 解析证书摘要。
///
/// # Errors
///
/// DER 结构不合法时返回 `bad_certificate`。
pub(crate) fn summarize(der: &[u8]) -> RedfishResult<CertSummary> {
    let (cert, _rest) = next_tlv(der)?;
    let (tbs, _sig) = next_tlv(cert.value)?;
    let items = children(tbs.value)?;
    // 跳过（可选的显式版本号、）序列号与签名算法
    let skip = if items.first().is_some_and(|item| item.tag == 0xA0) {
        3
    } else {
        2
    };
    let mut fields = items.into_iter().skip(skip);
    let issuer = fields.next().ok_or_else(bad)?;
    let validity = fields.next().ok_or_else(bad)?;
    let subject = fields.next().ok_or_else(bad)?;
    let times = children(validity.value)?;
    Ok(CertSummary {
        subject: name(subject.value)?,
        issuer: name(issuer.value)?,
        not_before: times.first().map(time).transpose()?.unwrap_or_default(),
        not_after: times.get(1).map(time).transpose()?.unwrap_or_default(),
    })
}

/// `Name` → `CN=…, O=…`。
///
/// # Errors
///
/// 结构不合法时返回错误。
fn name(input: &[u8]) -> RedfishResult<String> {
    let mut parts = Vec::new();
    for set in children(input)? {
        for attribute in children(set.value)? {
            let pair = children(attribute.value)?;
            let (Some(oid), Some(value)) = (pair.first(), pair.get(1)) else {
                return Err(bad());
            };
            parts.push(format!(
                "{}={}",
                attribute_name(oid.value),
                String::from_utf8_lossy(value.value)
            ));
        }
    }
    Ok(parts.join(", "))
}

/// 常见属性 OID 的简称；其它 OID 以点分形式显示。
fn attribute_name(oid: &[u8]) -> String {
    match oid {
        [0x55, 0x04, 0x03] => "CN".to_owned(),
        [0x55, 0x04, 0x06] => "C".to_owned(),
        [0x55, 0x04, 0x07] => "L".to_owned(),
        [0x55, 0x04, 0x08] => "ST".to_owned(),
        [0x55, 0x04, 0x0A] => "O".to_owned(),
        [0x55, 0x04, 0x0B] => "OU".to_owned(),
        other => dotted(other),
    }
}

/// OID 字节 → 点分文本。
fn dotted(oid: &[u8]) -> String {
    let mut arcs: Vec<u64> = Vec::new();
    let mut current = 0_u64;
    for byte in oid {
        current = current
            .checked_mul(128)
            .and_then(|shifted| shifted.checked_add(u64::from(byte & 0x7F)))
            .unwrap_or(u64::MAX);
        if byte & 0x80 == 0 {
            arcs.push(current);
            current = 0;
        }
    }
    let mut parts = Vec::new();
    if let Some((&head, tail)) = arcs.split_first() {
        let top = head.min(80).checked_div(40).unwrap_or(0).min(2);
        parts.push(top.to_string());
        parts.push(head.saturating_sub(top.saturating_mul(40)).to_string());
        parts.extend(tail.iter().map(ToString::to_string));
    }
    parts.join(".")
}

/// `UTCTime` / `GeneralizedTime` → `YYYY-MM-DDTHH:MM:SSZ`。
///
/// # Errors
///
/// 格式不符时返回错误。
fn time(item: &Tlv<'_>) -> RedfishResult<String> {
    let text = std::str::from_utf8(item.value).map_err(|_error| bad())?;
    let digits = text.trim_end_matches('Z');
    let full = match item.tag {
        0x17 => {
            let year = digits.get(..2).ok_or_else(bad)?;
            let century = if year < "50" { "20" } else { "19" };
            format!("{century}{digits}")
        }
        0x18 => digits.to_owned(),
        _other => return Err(bad()),
    };
    let part = |from: usize, to: usize| full.get(from..to).ok_or_else(bad);
    Ok(format!(
        "{}-{}-{}T{}:{}:{}Z",
        part(0, 4)?,
        part(4, 6)?,
        part(6, 8)?,
        part(8, 10)?,
        part(10, 12)?,
        part(12, 14)?
    ))
}

#[cfg(test)]
mod tests;
