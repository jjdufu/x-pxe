//! 主密钥与信封加密：每个 BMC 密码用 XChaCha20-Poly1305 加密，
//! `AAD = bmc_object_id（8 字节大端）‖ key_id`，防止密文在记录间挪用。

use std::fmt::{Debug, Formatter};
use std::path::Path;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use chacha20poly1305::aead::{Aead as _, KeyInit as _, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use rusqlite::{Connection, Transaction};
use sha2::{Digest as _, Sha256};

use crate::sqlval::{exec, query_opt, query_vec, random};
use crate::{Store, StoreError, StoreResult};

/// `meta` 表中登记主密钥标识的键。
const KEY_ID_META: &str = "master_key_id";

/// 32 字节主密钥，只存在于内存。`Debug` 只显示 `key_id`。
#[derive(Clone, PartialEq, Eq)]
pub struct MasterKey {
    /// 原始密钥字节。
    bytes: Box<[u8; 32]>,
}

impl Debug for MasterKey {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MasterKey")
            .field("key_id", &self.key_id())
            .finish_non_exhaustive()
    }
}

impl MasterKey {
    /// 由 32 字节构造。
    #[must_use]
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self {
            bytes: Box::new(bytes),
        }
    }

    /// 用系统 CSPRNG 生成新主密钥。
    ///
    /// # Errors
    ///
    /// 随机源不可用时返回错误。
    pub fn generate() -> StoreResult<Self> {
        Ok(Self::from_bytes(random::<32>()?))
    }

    /// 解析文件内容：恰好 32 字节原始数据，或（去掉首尾空白后）32 字节的 base64。
    ///
    /// # Errors
    ///
    /// 长度或编码不对时返回错误。
    pub fn parse(content: &[u8]) -> StoreResult<Self> {
        if let Ok(bytes) = <[u8; 32]>::try_from(content) {
            return Ok(Self::from_bytes(bytes));
        }
        let text = std::str::from_utf8(content)
            .map_err(|_utf8| {
                StoreError::MasterKey("既不是 32 字节原始数据也不是 base64".to_owned())
            })?
            .trim();
        let decoded = STANDARD.decode(text).map_err(|_decode| {
            StoreError::MasterKey("既不是 32 字节原始数据也不是 base64".to_owned())
        })?;
        let bytes = <[u8; 32]>::try_from(decoded.as_slice()).map_err(|_len| {
            StoreError::MasterKey(format!("长度必须为 32 字节，实际 {}", decoded.len()))
        })?;
        Ok(Self::from_bytes(bytes))
    }

    /// 读取主密钥文件，同时返回权限告警（宽于 `0400`/`0440` 时）。
    ///
    /// # Errors
    ///
    /// 文件缺失、不可读或内容不合法时返回错误。
    pub fn load(path: &Path) -> StoreResult<(Self, Vec<String>)> {
        let content = std::fs::read(path)
            .map_err(|error| StoreError::MasterKey(format!("{}：{error}", path.display())))?;
        let key = Self::parse(&content)?;
        let mut warnings = Vec::new();
        let mode = mode_bits(path)?;
        if mode & !0o440 != 0 {
            warnings.push(format!(
                "主密钥文件 {} 权限为 {mode:o}，宽于 0400/0440",
                path.display()
            ));
        }
        Ok((key, warnings))
    }

    /// `key_id`：主密钥 SHA-256 的前 8 字节（十六进制）。
    #[must_use]
    pub fn key_id(&self) -> String {
        let digest = Sha256::digest(self.bytes.as_slice());
        hex::encode(digest.get(..8).unwrap_or_default())
    }

    /// base64 文本（供 `xpxe-control` 生成密钥文件）。
    #[must_use]
    pub fn to_base64(&self) -> String {
        STANDARD.encode(self.bytes.as_slice())
    }

    /// 加密一条 BMC 密码，返回 `(nonce, ciphertext)`。
    ///
    /// # Errors
    ///
    /// 随机源或加密失败时返回错误。
    fn seal(&self, bmc_object_id: i64, plaintext: &[u8]) -> StoreResult<Sealed> {
        let nonce_bytes = random::<24>()?;
        let cipher = self.cipher()?;
        let aad = self.aad(bmc_object_id);
        let ciphertext = cipher
            .encrypt(
                &XNonce::from(nonce_bytes),
                Payload {
                    msg: plaintext,
                    aad: &aad,
                },
            )
            .map_err(|_aead| StoreError::Crypto)?;
        Ok(Sealed {
            nonce: nonce_bytes.to_vec(),
            ciphertext,
        })
    }

    /// 解密一条 BMC 密码。
    ///
    /// # Errors
    ///
    /// 密文被篡改、挪用或用错密钥时返回 [`StoreError::Crypto`]。
    fn open(&self, bmc_object_id: i64, sealed: &Sealed) -> StoreResult<Vec<u8>> {
        let nonce_bytes =
            <[u8; 24]>::try_from(sealed.nonce.as_slice()).map_err(|_len| StoreError::Crypto)?;
        let aad = self.aad(bmc_object_id);
        self.cipher()?
            .decrypt(
                &XNonce::from(nonce_bytes),
                Payload {
                    msg: &sealed.ciphertext,
                    aad: &aad,
                },
            )
            .map_err(|_aead| StoreError::Crypto)
    }

    /// AEAD 实例。
    ///
    /// # Errors
    ///
    /// 密钥长度不对时返回错误（不会发生）。
    fn cipher(&self) -> StoreResult<XChaCha20Poly1305> {
        XChaCha20Poly1305::new_from_slice(self.bytes.as_slice()).map_err(|_len| StoreError::Crypto)
    }

    /// 附加认证数据。
    fn aad(&self, bmc_object_id: i64) -> Vec<u8> {
        let mut aad = bmc_object_id.to_be_bytes().to_vec();
        aad.extend_from_slice(self.key_id().as_bytes());
        aad
    }
}

/// 一条密文。
#[derive(Debug)]
struct Sealed {
    /// 24 字节随机数。
    nonce: Vec<u8>,
    /// 密文（含认证标签）。
    ciphertext: Vec<u8>,
}

/// 行 → `(bmc_object_id, 密文)`。
///
/// # Errors
///
/// 列类型不符时返回错误。
fn sealed_row(row: &rusqlite::Row<'_>) -> StoreResult<(i64, Sealed)> {
    Ok((
        row.get(0)?,
        Sealed {
            nonce: row.get(1)?,
            ciphertext: row.get(2)?,
        },
    ))
}

/// 文件权限位（非 Unix 平台返回 0）。
///
/// # Errors
///
/// 读取元数据失败时返回错误。
fn mode_bits(path: &Path) -> StoreResult<u32> {
    use std::os::unix::fs::PermissionsExt as _;
    Ok(std::fs::metadata(path)?.permissions().mode() & 0o7777)
}

/// 首次打开时登记 `key_id`，之后核对。
///
/// # Errors
///
/// 与登记不符时返回 [`StoreError::KeyMismatch`]。
pub(crate) fn check_key_id(conn: &mut Connection, key: &MasterKey) -> StoreResult<()> {
    let tx = conn.transaction()?;
    let stored = query_opt(
        &tx,
        "SELECT value FROM meta WHERE key = ?1",
        [KEY_ID_META],
        |row| Ok(row.get::<_, String>(0)?),
    )?;
    match stored {
        Some(id) if id == key.key_id() => {}
        Some(_) => return Err(StoreError::KeyMismatch),
        None => {
            exec(
                &tx,
                "INSERT INTO meta (key, value) VALUES (?1, ?2)",
                [KEY_ID_META, key.key_id().as_str()],
            )?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// 用新密钥重加密全部密文并更新登记。
///
/// # Errors
///
/// 任一密文无法用旧密钥解开时返回错误。
pub(crate) fn rotate(tx: &Transaction<'_>, old: &MasterKey, new: &MasterKey) -> StoreResult<()> {
    let rows = query_vec(
        tx,
        "SELECT bmc_object_id, nonce, ciphertext FROM bmc_secrets",
        [],
        sealed_row,
    )?;
    for (id, sealed) in rows {
        let plain = old.open(id, &sealed)?;
        let resealed = new.seal(id, &plain)?;
        exec(
            tx,
            "UPDATE bmc_secrets SET key_id = ?2, nonce = ?3, ciphertext = ?4 WHERE bmc_object_id = ?1",
            rusqlite::params![id, new.key_id(), resealed.nonce, resealed.ciphertext],
        )?;
    }
    exec(
        tx,
        "UPDATE meta SET value = ?2 WHERE key = ?1",
        [KEY_ID_META, new.key_id().as_str()],
    )?;
    Ok(())
}

/// BMC 密码密文的读写句柄。
#[derive(Debug, Clone, Copy)]
pub struct Secrets<'store> {
    /// 所属存储。
    store: &'store Store,
    /// 当前主密钥。
    key: &'store MasterKey,
}

impl<'store> Secrets<'store> {
    /// 构造句柄。
    pub(crate) const fn new(store: &'store Store, key: &'store MasterKey) -> Self {
        Self { store, key }
    }

    /// 写入或替换一条 BMC 密码。
    ///
    /// # Errors
    ///
    /// 加密或写入失败时返回错误。
    pub fn put(&self, bmc_object_id: i64, password: &str) -> StoreResult<()> {
        let sealed = self.key.seal(bmc_object_id, password.as_bytes())?;
        self.store.write(|tx| {
            exec(tx,
                "INSERT INTO bmc_secrets (bmc_object_id, key_id, nonce, ciphertext) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT (bmc_object_id) DO UPDATE SET key_id = excluded.key_id,
                   nonce = excluded.nonce, ciphertext = excluded.ciphertext",
                rusqlite::params![bmc_object_id, self.key.key_id(), sealed.nonce, sealed.ciphertext],
            )?;
            Ok(())
        })
    }

    /// 读取并解密一条 BMC 密码。
    ///
    /// # Errors
    ///
    /// 解密失败时返回 [`StoreError::Crypto`]。
    pub fn get(&self, bmc_object_id: i64) -> StoreResult<Option<String>> {
        let row = self.store.read(|conn| {
            query_opt(
                conn,
                "SELECT bmc_object_id, nonce, ciphertext FROM bmc_secrets WHERE bmc_object_id = ?1",
                [bmc_object_id],
                sealed_row,
            )
        })?;
        let Some((_id, sealed)) = row else {
            return Ok(None);
        };
        let plain = self.key.open(bmc_object_id, &sealed)?;
        String::from_utf8(plain)
            .map(Some)
            .map_err(|_utf8| StoreError::Crypto)
    }

    /// 删除一条 BMC 密码。
    ///
    /// # Errors
    ///
    /// 写入失败时返回错误。
    pub fn delete(&self, bmc_object_id: i64) -> StoreResult<bool> {
        self.store.write(|tx| {
            Ok(tx.execute(
                "DELETE FROM bmc_secrets WHERE bmc_object_id = ?1",
                [bmc_object_id],
            )? > 0)
        })
    }
}
