//! 将明文口令即时转为 `$6$rounds=656000$` 哈希并丢弃明文。

use ring::rand::{SecureRandom as _, SystemRandom};
use sha_crypt::password_hash::PasswordHasher as _;
use sha_crypt::{Algorithm, Params, ShaCrypt};
use xpxe_domain::CryptSecret;

use crate::RenderError;

/// 默认 sha-crypt 迭代次数。
const ROUNDS: u32 = 656_000;
/// 盐长度（字节）。
const SALT_LEN: usize = 16;

/// 用 sha512crypt（`rounds=656000`）哈希口令。
///
/// # Errors
///
/// 参数非法、取盐失败或哈希失败时返回错误。
pub fn hash_password(plain: &str) -> Result<CryptSecret, RenderError> {
    if plain.is_empty() {
        return Err(RenderError::new(
            "empty_password",
            "口令不能为空".to_owned(),
        ));
    }
    let params = Params::new(ROUNDS)
        .map_err(|error| RenderError::new("hash_params", format!("无法设置 rounds：{error}")))?;
    let hasher = ShaCrypt::new(Algorithm::Sha512Crypt, params);
    let salt = random_salt()?;
    let hash = hasher
        .hash_password_with_salt(plain.as_bytes(), &salt)
        .map_err(|error| RenderError::new("hash_failed", format!("哈希失败：{error}")))?;
    CryptSecret::parse(hash.as_str())
        .map_err(|error| RenderError::new("hash_format", format!("生成的哈希无法解析：{error}")))
}

/// 16 字节随机盐。
///
/// # Errors
///
/// 系统随机源失败时返回错误。
fn random_salt() -> Result<[u8; SALT_LEN], RenderError> {
    let mut salt = [0_u8; SALT_LEN];
    SystemRandom::new()
        .fill(&mut salt)
        .map_err(|error| RenderError::new("rng", format!("生成盐失败：{error}")))?;
    Ok(salt)
}
