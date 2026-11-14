//! SSH 公钥结构校验。

use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD};

use super::{ParseError, SshPublicKey, new_ssh_public_key};

/// 允许的密钥类型。
const ALLOWED_TYPES: &[&str] = &[
    "ssh-ed25519",
    "ecdsa-sha2-nistp256",
    "ecdsa-sha2-nistp384",
    "ecdsa-sha2-nistp521",
    "ssh-rsa",
    "sk-ssh-ed25519@openssh.com",
    "sk-ecdsa-sha2-nistp256@openssh.com",
];

/// 解析一行 SSH 公钥。
///
/// # Errors
///
/// 见 [`SshPublicKey::parse`]。
pub(crate) fn parse(raw: &str) -> Result<SshPublicKey, ParseError> {
    if raw.chars().any(char::is_control) {
        return Err(ssh_err("control_char"));
    }
    let (kind, payload, comment) = split_key(raw)?;
    if !ALLOWED_TYPES.contains(&kind) {
        return Err(ssh_err("bad_type"));
    }
    if let Some(text) = comment
        && !text
            .chars()
            .all(|ch| ch.is_ascii() && !ch.is_ascii_control())
    {
        return Err(ssh_err("bad_comment"));
    }
    let decoded = decode_base64(payload)?;
    let mut cursor = Cursor {
        rest: decoded.as_slice(),
    };
    let embedded = cursor.ssh_str().and_then(to_utf8)?;
    if embedded != kind {
        return Err(ssh_err("type_mismatch"));
    }
    validate_body(kind, &mut cursor)?;
    if !cursor.rest.is_empty() {
        return Err(ssh_err("trailing"));
    }
    Ok(new_ssh_public_key(raw.to_owned()))
}

/// 一行公钥拆出的类型、payload 与可选注释。
type KeyParts<'text> = (&'text str, &'text str, Option<&'text str>);

/// 按空格拆成类型、payload 与可选注释（注释可含空格）。
///
/// # Errors
///
/// 失败时返回对应错误。
fn split_key(raw: &str) -> Result<KeyParts<'_>, ParseError> {
    let Some((kind, after_type)) = raw.split_once(' ') else {
        return Err(ssh_err("bad_format"));
    };
    if kind.is_empty() {
        return Err(ssh_err("bad_type"));
    }
    let remainder = after_type.trim_start_matches(' ');
    let (payload, comment) = split_payload(remainder);
    if payload.is_empty() {
        return Err(ssh_err("bad_format"));
    }
    Ok((kind, payload, comment))
}

/// 从剩余文本取出 payload 与可选注释。
///
/// # Errors
///
/// 失败时返回对应错误。
fn split_payload(rest: &str) -> (&str, Option<&str>) {
    match rest.split_once(' ') {
        Some((payload, comment)) => (payload, Some(comment)),
        None => (rest, None),
    }
}

/// 解码标准 base64，允许缺少填充。
///
/// # Errors
///
/// 失败时返回对应错误。
fn decode_base64(payload: &str) -> Result<Vec<u8>, ParseError> {
    STANDARD
        .decode(payload)
        .or_else(|_error| STANDARD_NO_PAD.decode(payload))
        .map_err(|_error| ssh_err("bad_base64"))
}

/// 按类型校验剩余字段。
///
/// # Errors
///
/// 失败时返回对应错误。
fn validate_body(kind: &str, cursor: &mut Cursor<'_>) -> Result<(), ParseError> {
    match kind {
        "ssh-ed25519" | "sk-ssh-ed25519@openssh.com" => validate_ed25519(kind, cursor),
        "ecdsa-sha2-nistp256"
        | "ecdsa-sha2-nistp384"
        | "ecdsa-sha2-nistp521"
        | "sk-ecdsa-sha2-nistp256@openssh.com" => validate_ecdsa(kind, cursor),
        "ssh-rsa" => validate_rsa(cursor),
        _ => Err(ssh_err("bad_type")),
    }
}

/// 校验 Ed25519 或 sk-ed25519。
///
/// # Errors
///
/// 失败时返回对应错误。
fn validate_ed25519(kind: &str, cursor: &mut Cursor<'_>) -> Result<(), ParseError> {
    let key = cursor.ssh_str()?;
    if key.len() != 32 {
        return Err(ssh_err("bad_key"));
    }
    if kind == "sk-ssh-ed25519@openssh.com" {
        let app = cursor.ssh_str()?;
        if app.is_empty() {
            return Err(ssh_err("bad_key"));
        }
    }
    Ok(())
}

/// 校验 ECDSA 或 sk-ECDSA。
///
/// # Errors
///
/// 失败时返回对应错误。
fn validate_ecdsa(kind: &str, cursor: &mut Cursor<'_>) -> Result<(), ParseError> {
    let curve = cursor.ssh_str().and_then(to_utf8)?;
    let expected = ecdsa_curve(kind)?;
    if curve != expected {
        return Err(ssh_err("type_mismatch"));
    }
    let point = cursor.ssh_str()?;
    if point.is_empty() {
        return Err(ssh_err("bad_key"));
    }
    if kind == "sk-ecdsa-sha2-nistp256@openssh.com" {
        let app = cursor.ssh_str()?;
        if app.is_empty() {
            return Err(ssh_err("bad_key"));
        }
    }
    Ok(())
}

/// 由密钥类型得到期望曲线名。
///
/// # Errors
///
/// 失败时返回对应错误。
fn ecdsa_curve(kind: &str) -> Result<&'static str, ParseError> {
    match kind {
        "ecdsa-sha2-nistp256" | "sk-ecdsa-sha2-nistp256@openssh.com" => Ok("nistp256"),
        "ecdsa-sha2-nistp384" => Ok("nistp384"),
        "ecdsa-sha2-nistp521" => Ok("nistp521"),
        _ => Err(ssh_err("bad_type")),
    }
}

/// 校验 RSA：模数至少 3072 位。
///
/// # Errors
///
/// 失败时返回对应错误。
fn validate_rsa(cursor: &mut Cursor<'_>) -> Result<(), ParseError> {
    let exponent = cursor.ssh_str()?;
    let modulus = cursor.ssh_str()?;
    if exponent.is_empty() || modulus.is_empty() {
        return Err(ssh_err("bad_key"));
    }
    if mpint_bit_len(modulus)? < 3072 {
        return Err(ssh_err("rsa_too_short"));
    }
    Ok(())
}

/// 计算 SSH mpint 的位长。
///
/// # Errors
///
/// 失败时返回对应错误。
fn mpint_bit_len(bytes: &[u8]) -> Result<u32, ParseError> {
    let mut start = 0_usize;
    while start < bytes.len() && bytes.get(start) == Some(&0) {
        start = start.checked_add(1).ok_or_else(|| ssh_err("bad_key"))?;
    }
    let rest = bytes.get(start..).unwrap_or(&[]);
    if rest.is_empty() {
        return Ok(0);
    }
    let first = *rest.first().ok_or_else(|| ssh_err("bad_key"))?;
    let first_bits = 8_u32
        .checked_sub(first.leading_zeros())
        .ok_or_else(|| ssh_err("bad_key"))?;
    let tail = rest
        .len()
        .checked_sub(1)
        .ok_or_else(|| ssh_err("bad_key"))?;
    let tail_bits = u32::try_from(tail)
        .ok()
        .and_then(|len| len.checked_mul(8))
        .ok_or_else(|| ssh_err("bad_key"))?;
    first_bits
        .checked_add(tail_bits)
        .ok_or_else(|| ssh_err("bad_key"))
}

/// 字节切片转 UTF-8。
///
/// # Errors
///
/// 失败时返回对应错误。
fn to_utf8(bytes: &[u8]) -> Result<&str, ParseError> {
    std::str::from_utf8(bytes).map_err(|_error| ssh_err("bad_key"))
}

/// 统一的 SSH 错误。
///
/// # Errors
///
/// 失败时返回对应错误。
const fn ssh_err(code: &'static str) -> ParseError {
    ParseError::new("SshPublicKey", code)
}

/// 读取 SSH 线格式字段的游标。
struct Cursor<'buf> {
    /// 尚未读取的字节。
    rest: &'buf [u8],
}

impl<'buf> Cursor<'buf> {
    /// 读取 `len` 字节。
    ///
    /// # Errors
    ///
    /// 失败时返回对应错误。
    fn take(&mut self, len: usize) -> Result<&'buf [u8], ParseError> {
        let (head, tail) = self
            .rest
            .split_at_checked(len)
            .ok_or_else(|| ssh_err("bad_key"))?;
        self.rest = tail;
        Ok(head)
    }

    /// 读取一个长度前缀字符串。
    ///
    /// # Errors
    ///
    /// 失败时返回对应错误。
    fn ssh_str(&mut self) -> Result<&'buf [u8], ParseError> {
        let len_bytes = self.take(4)?;
        let len_arr = <[u8; 4]>::try_from(len_bytes).map_err(|_error| ssh_err("bad_key"))?;
        let len =
            usize::try_from(u32::from_be_bytes(len_arr)).map_err(|_error| ssh_err("bad_key"))?;
        self.take(len)
    }
}
