//! 测试公共件。

use xpxe_assets::TrustStore;
use xpxe_domain::Timestamp;
use xpxe_test_support::fixture_bytes;

/// 2026-11-18T00:00:00Z。
pub(crate) const NOW: Timestamp = Timestamp::from_unix_seconds(1_795_478_400);

/// 读取夹具。
///
/// # Errors
///
/// 读取失败时返回错误。
pub(crate) fn fixture(path: &str) -> std::io::Result<Vec<u8>> {
    fixture_bytes(env!("CARGO_MANIFEST_DIR"), path)
}

/// 读取文本夹具。
///
/// # Errors
///
/// 读取失败或不是 UTF-8 时返回错误。
pub(crate) fn fixture_text(path: &str) -> Result<String, Box<dyn std::error::Error>> {
    Ok(String::from_utf8(fixture(path)?)?)
}

/// 测试密钥信任根；`expired` 为真时白名单过期时间在过去。
///
/// # Errors
///
/// 加载失败时返回错误。
pub(crate) fn test_trust(expired: bool) -> Result<TrustStore, Box<dyn std::error::Error>> {
    let keys = fixture_text("pgp/test-keyring.asc")?;
    let pins = if expired {
        fixture_text("pgp/test-fingerprints-expired.txt")?
    } else {
        fixture_text("pgp/test-fingerprints.txt")?
    };
    Ok(TrustStore::from_parts(&[&keys], &pins)?)
}

/// 新建多线程运行时。
///
/// # Errors
///
/// 创建失败时返回错误。
pub(crate) fn runtime() -> std::io::Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
}

/// 取错误码。
pub(crate) fn code<T>(result: &Result<T, xpxe_assets::AssetsError>) -> Option<&'static str> {
    result.as_ref().err().map(xpxe_assets::AssetsError::code)
}
