//! 主密钥与信封加密。

use std::fs;
use std::os::unix::fs::PermissionsExt as _;

use rusqlite::Connection;
use xpxe_store::{DB_FILE, MasterKey, Store, StoreError};
use xpxe_test_support::{TempDataDir, TestResult, ensure, ensure_eq};

use crate::support::{key_a, key_b, open};

/// 用错主密钥拒绝打开。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn wrong_master_key_is_refused() -> TestResult {
    let dir = TempDataDir::new()?;
    drop(open(dir.path())?);
    ensure_eq(
        &Store::open(dir.path(), key_b()).map(|_store| ()),
        &Err(StoreError::KeyMismatch),
        "错误主密钥",
    )?;
    ensure(open(dir.path()).is_ok(), "正确主密钥")?;
    Ok(())
}

/// 密文挪到另一条记录后解密失败。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn moved_ciphertext_fails() -> TestResult {
    let dir = TempDataDir::new()?;
    let store = open(dir.path())?;
    store.secrets().put(1, "bmc-one-password")?;
    store.secrets().put(2, "bmc-two-password")?;
    ensure_eq(
        &store.secrets().get(1)?,
        &Some("bmc-one-password".to_owned()),
        "可读",
    )?;
    let raw = Connection::open(dir.path().join(DB_FILE))?;
    let _moved = raw.execute(
        "UPDATE bmc_secrets SET nonce = (SELECT nonce FROM bmc_secrets WHERE bmc_object_id = 1),
           ciphertext = (SELECT ciphertext FROM bmc_secrets WHERE bmc_object_id = 1)
         WHERE bmc_object_id = 2",
        [],
    )?;
    ensure_eq(
        &store.secrets().get(2),
        &Err(StoreError::Crypto),
        "挪用被拒",
    )?;
    ensure_eq(&store.secrets().get(3)?, &None, "不存在")?;
    Ok(())
}

/// 轮换后旧密钥失效、新密钥可读。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn rotation_switches_keys() -> TestResult {
    let dir = TempDataDir::new()?;
    {
        let store = open(dir.path())?;
        store.secrets().put(5, "rotate-me-please")?;
    }
    Store::rotate_master_key(dir.path(), key_a(), &key_b())?;
    ensure_eq(
        &open(dir.path()).map(|_store| ()),
        &Err(StoreError::KeyMismatch),
        "旧密钥失效",
    )?;
    let store = Store::open(dir.path(), key_b())?;
    ensure_eq(
        &store.secrets().get(5)?,
        &Some("rotate-me-please".to_owned()),
        "新密钥可读",
    )?;
    Ok(())
}

/// 主密钥文件：原始 32 字节或 base64；权限过宽告警；长度不对拒绝。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn master_key_file_formats() -> TestResult {
    let dir = TempDataDir::new()?;
    let raw_path = dir.path().join("raw.key");
    fs::write(&raw_path, [7_u8; 32])?;
    fs::set_permissions(&raw_path, fs::Permissions::from_mode(0o400))?;
    let (raw_key, raw_warnings) = MasterKey::load(&raw_path)?;
    ensure_eq(&raw_key, &key_a(), "原始字节")?;
    ensure(raw_warnings.is_empty(), "0400 无告警")?;
    let b64_path = dir.path().join("b64.key");
    fs::write(&b64_path, format!("{}\n", key_a().to_base64()))?;
    fs::set_permissions(&b64_path, fs::Permissions::from_mode(0o644))?;
    let (b64_key, b64_warnings) = MasterKey::load(&b64_path)?;
    ensure_eq(&b64_key, &key_a(), "base64")?;
    ensure_eq(&b64_warnings.len(), &1, "0644 告警")?;
    let short_path = dir.path().join("short.key");
    fs::write(&short_path, [1_u8; 16])?;
    ensure(
        matches!(MasterKey::load(&short_path), Err(StoreError::MasterKey(_))),
        "长度不对",
    )?;
    ensure(
        matches!(
            MasterKey::load(&dir.path().join("missing.key")),
            Err(StoreError::MasterKey(_))
        ),
        "缺失",
    )?;
    ensure(!format!("{:?}", key_a()).contains("7, 7"), "Debug 不泄露")?;
    Ok(())
}

/// 数据目录里找不到主密钥内容（原始或 base64），BMC 密码也不以明文出现。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn data_dir_has_no_key_material() -> TestResult {
    let dir = TempDataDir::new()?;
    let raw_key: [u8; 32] = [
        0x3a, 0x91, 0x07, 0xc4, 0x5e, 0x22, 0xf0, 0x18, 0x66, 0xab, 0x0d, 0x7c, 0x93, 0x41, 0xe8,
        0x2f, 0xb5, 0x10, 0x4c, 0xd9, 0x77, 0x08, 0x9e, 0x63, 0xc1, 0x2a, 0xf7, 0x54, 0x86, 0x3d,
        0xe0, 0x19,
    ];
    let key = MasterKey::from_bytes(raw_key);
    let needle_b64 = key.to_base64();
    ensure_eq(
        &MasterKey::parse(needle_b64.as_bytes())?,
        &key,
        "base64 往返",
    )?;
    ensure(MasterKey::generate()? != key, "随机生成")?;
    {
        let store = Store::open(dir.path(), key)?;
        store.secrets().put(1, "plain-bmc-secret")?;
        store.backup(&dir.path().join("backup.db"))?;
    }
    for entry in fs::read_dir(dir.path())? {
        let bytes = fs::read(entry?.path())?;
        ensure(!contains(&bytes, needle_b64.as_bytes()), "无 base64 主密钥")?;
        ensure(!contains(&bytes, &raw_key), "无原始主密钥")?;
        ensure(!contains(&bytes, b"plain-bmc-secret"), "无明文密码")?;
    }
    Ok(())
}

/// 字节串包含。
fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}
