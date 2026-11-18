//! Debian 签名链（离线夹具）：真实 InRelease/SHA256SUMS 通过；篡改、换密钥、过期、缺条目均失败。

use xpxe_assets::{
    FetchMeta, NetbootFiles, StoredFile, TrustStore, finish_netboot, netboot_expectations,
    verify_release,
};
use xpxe_store::AssetSource;
use xpxe_test_support::{TempDataDir, TestResult, ensure, ensure_eq};

use crate::support::{NOW, code, fixture, fixture_text, runtime, test_trust};

/// 真实链：InRelease → SHA256SUMS → linux/initrd 期望哈希。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn real_chain_yields_expectations() -> TestResult {
    let trust = TrustStore::debian()?;
    let expect = netboot_expectations(
        Some(&trust),
        &fixture("debian/InRelease")?,
        &fixture("debian/SHA256SUMS")?,
        NOW,
    )?;
    ensure_eq(
        expect.linux.as_str(),
        "2b2358b37674d2505350528875bb17afae2a36522a9e8a9417eaca65a7da0e08",
        "linux",
    )?;
    ensure(expect.initrd.starts_with("57303d15"), "initrd")?;
    ensure(expect.release.signer.is_some(), "signer")?;
    // 2026-09-12 的 InRelease 在 2026-11-18 已早于 30 天
    ensure(
        expect
            .release
            .warnings
            .iter()
            .any(|text| text.contains("30 天")),
        "stale warning",
    )?;
    Ok(())
}

/// 篡改 `InRelease` 正文任一字节 → `bad_signature`；篡改 `SHA256SUMS` → `hash_mismatch`。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn tampering_fails() -> TestResult {
    let trust = TrustStore::debian()?;
    let original = fixture_text("debian/InRelease")?;
    let tampered = original.replacen("Suite: stable", "Suite: stabke", 1);
    ensure(tampered != original, "fixture changed")?;
    let result = verify_release(Some(&trust), tampered.as_bytes(), NOW);
    ensure_eq(&code(&result), &Some("bad_signature"), "inrelease")?;
    let mut sums = fixture("debian/SHA256SUMS")?;
    if let Some(byte) = sums.get_mut(10) {
        *byte ^= 0x01;
    }
    let sums_result = netboot_expectations(Some(&trust), original.as_bytes(), &sums, NOW);
    ensure_eq(&code(&sums_result), &Some("hash_mismatch"), "sums")?;
    Ok(())
}

/// 换签名密钥、过期密钥、`SHA256SUMS` 不在 `InRelease` 中。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn wrong_expired_and_missing() -> TestResult {
    let rsa = fixture("pgp/InRelease-rsa")?;
    let debian = TrustStore::debian()?;
    ensure_eq(
        &code(&verify_release(Some(&debian), &rsa, NOW)),
        &Some("untrusted_signer"),
        "wrong key",
    )?;
    let expired = test_trust(true)?;
    ensure_eq(
        &code(&verify_release(Some(&expired), &rsa, NOW)),
        &Some("key_expired"),
        "expired",
    )?;
    let trust = test_trust(false)?;
    let sums = fixture("debian/SHA256SUMS")?;
    ensure(
        netboot_expectations(Some(&trust), &rsa, &sums, NOW).is_ok(),
        "test chain ok",
    )?;
    let missing = fixture("pgp/InRelease-missing")?;
    ensure_eq(
        &code(&netboot_expectations(Some(&trust), &missing, &sums, NOW)),
        &Some("not_in_release"),
        "missing",
    )?;
    Ok(())
}

/// 不签名镜像：跳过签名但给出警告；内联签名仍被剥离。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn unsigned_mirror_mode() -> TestResult {
    let release = verify_release(None, &fixture("pgp/InRelease-rsa")?, NOW)?;
    ensure(release.signer.is_none(), "no signer")?;
    ensure(
        release.release.sha256_of("main/other").is_some(),
        "parsed body",
    )?;
    ensure(
        release.warnings.iter().any(|text| text.contains("不签名")),
        "warning",
    )?;
    Ok(())
}

/// 固件 `SHA256SUMS.sign`（Debian CD 密钥）与测试分离签名。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn detached_signatures() -> TestResult {
    let debian = TrustStore::debian()?;
    let sums = fixture("firmware/SHA256SUMS")?;
    let signer = debian.verify_detached(&sums, &fixture("firmware/SHA256SUMS.sign")?, NOW)?;
    ensure_eq(
        signer.primary_fingerprint.as_str(),
        "DF9B9C49EAA9298432589D76DA87E80D6294BE9B",
        "cd key",
    )?;
    let mut tampered = sums.clone();
    if let Some(byte) = tampered.first_mut() {
        *byte ^= 0x01;
    }
    ensure_eq(
        &code(&debian.verify_detached(&tampered, &fixture("firmware/SHA256SUMS.sign")?, NOW)),
        &Some("bad_signature"),
        "tampered",
    )?;
    let trust = test_trust(false)?;
    ensure(
        trust
            .verify_detached(&sums, &fixture("pgp/blob.sig")?, NOW)
            .is_ok(),
        "test detached",
    )?;
    Ok(())
}

/// 链尾：成员文件哈希核对、ABI 提取、清单落盘。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn finish_writes_manifest() -> TestResult {
    let dir = TempDataDir::new()?;
    let head = fixture("debian/linux-head.bin")?;
    let stored = |name: &str, data: &[u8]| -> TestResult<StoredFile> {
        let sha = hex::encode(<sha2::Sha256 as sha2::Digest>::digest(data));
        let path = dir.path().join(&sha);
        std::fs::write(&path, data)?;
        Ok(StoredFile {
            meta: FetchMeta {
                final_url: format!("file:///{name}"),
                etag: None,
                last_modified: None,
                sha256: sha,
                size: u64::try_from(data.len())?,
            },
            path,
        })
    };
    let linux = stored("linux", &head)?;
    let initrd = stored("initrd.gz", b"initrd")?;
    let trust = test_trust(false)?;
    let rsa = fixture("pgp/InRelease-rsa")?;
    let mut expect = netboot_expectations(Some(&trust), &rsa, &fixture("debian/SHA256SUMS")?, NOW)?;
    expect.linux.clone_from(&linux.meta.sha256);
    expect.initrd.clone_from(&initrd.meta.sha256);
    let files = NetbootFiles {
        inrelease: &rsa,
        sums: &[],
        linux: &linux,
        initrd: &initrd,
    };
    let asset = runtime()?.block_on(finish_netboot(
        dir.path(),
        &expect,
        files,
        (AssetSource::Upload, None),
    ))?;
    ensure_eq(
        &asset.kernel_abi.as_deref(),
        &Some("6.12.107+deb13-amd64"),
        "abi",
    )?;
    let manifest = std::fs::read_to_string(dir.path().join(&asset.sha256))?;
    ensure(manifest.contains("abi 6.12.107+deb13-amd64"), "manifest")?;
    expect.initrd = "0".repeat(64);
    let tampered_files = NetbootFiles {
        inrelease: &rsa,
        sums: &[],
        linux: &linux,
        initrd: &initrd,
    };
    let bad = runtime()?.block_on(finish_netboot(
        dir.path(),
        &expect,
        tampered_files,
        (AssetSource::Upload, None),
    ));
    ensure_eq(&code(&bad), &Some("hash_mismatch"), "member mismatch")?;
    Ok(())
}
