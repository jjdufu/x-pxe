//! 获取与校验流程：iPXE（格式 + 主机白名单）、Debian netboot（`OpenPGP` 签名链 + ABI）、
//! 固件（分离签名）。在线获取与离线上传共用同一套校验。

use std::path::Path;

use serde_json::{Value, json};
use xpxe_domain::Timestamp;
use xpxe_store::AssetSource;

use crate::align::{Alignment, Compression, check_alignment, decompress_packages};
use crate::catalog::VerifiedAsset;
use crate::error::{AssetsError, AssetsResult, fail};
use crate::fetch::{Fetcher, FileTarget, Origin, StoredFile, io_error, store_bytes};
use crate::ipxe::{IpxeFormat, check_efi_x64, check_undionly};
use crate::kernel::kernel_version;
use crate::kind::AssetKind;
use crate::release::{Release, Sha256Sums};
use crate::trust::{Signer, TrustStore};

/// `InRelease`/`SHA256SUMS` 等元数据文件的大小上限。
const META_LIMIT: u64 = 16 * 1024 * 1024;
/// 读取内核头部的字节数。
const KERNEL_HEAD: usize = 64 * 1024;
/// netboot 内核在 `SHA256SUMS` 中的路径。
pub const NETBOOT_LINUX: &str = "./netboot/debian-installer/amd64/linux";
/// netboot initrd 在 `SHA256SUMS` 中的路径。
pub const NETBOOT_INITRD: &str = "./netboot/debian-installer/amd64/initrd.gz";
/// 安装器 `SHA256SUMS` 在 `InRelease` 中的路径。
pub const INSTALLER_SUMS: &str = "main/installer-amd64/current/images/SHA256SUMS";

/// 镜像中 debian-installer 的 `Packages.xz`。
pub const PACKAGES_XZ: &str = "main/debian-installer/binary-amd64/Packages.xz";
/// 镜像中 debian-installer 的 `Packages.gz`。
pub const PACKAGES_GZ: &str = "main/debian-installer/binary-amd64/Packages.gz";

/// 对齐检查报告（控制面按（资产，镜像）缓存 24 h）。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct AlignmentReport {
    /// 结论。
    pub alignment: Alignment,
    /// 镜像地址。
    pub mirror: String,
    /// 使用的 `Packages` 路径。
    pub packages_path: String,
    /// 镜像 `InRelease` 签名者（不签名镜像为空）。
    pub signer: Option<Signer>,
    /// 镜像 `InRelease` 日期。
    pub release_date: Option<u64>,
    /// 检查时间。
    pub checked_at: u64,
}

/// 镜像源。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mirror<'url> {
    /// 镜像根地址，例如 `http://deb.debian.org/debian`。
    pub url: &'url str,
    /// 发行版代号，例如 `trixie`。
    pub suite: &'url str,
    /// 是否校验签名；`false` 表示 Profile 上显式标注“镜像不签名”。
    pub signed: bool,
}

impl Mirror<'_> {
    /// `<镜像>/dists/<suite>`。
    fn dists(self) -> String {
        format!("{}/dists/{}", self.url.trim_end_matches('/'), self.suite)
    }
}

/// 下载来源。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source<'url> {
    /// 官方源（第 2 节白名单）。
    Official,
    /// 管理员填写的 HTTPS URL。
    Url(&'url str),
}

/// 校验 iPXE 二进制格式。
///
/// # Errors
///
/// 种类不是 iPXE 或格式不符时返回错误。
pub fn verify_ipxe(kind: AssetKind, data: &[u8]) -> AssetsResult<IpxeFormat> {
    match kind {
        AssetKind::IpxeUndionly => check_undionly(data),
        AssetKind::IpxeEfiX64 | AssetKind::IpxeSnponlyX64 => check_efi_x64(data),
        AssetKind::DebianNetbootAmd64 | AssetKind::DebianFirmwareCpio => Err(AssetsError::new(
            "bad_kind",
            format!("{kind} 不是 iPXE 资产"),
        )),
    }
}

/// 已验证的 `InRelease`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedRelease {
    /// 解析结果。
    pub release: Release,
    /// 签名者；镜像显式标注“不签名”时为空。
    pub signer: Option<Signer>,
    /// 警告（如 `Date` 早于 30 天）。
    pub warnings: Vec<String>,
}

/// 验证 `InRelease`；`trust` 为空表示镜像显式标注“不签名”，只剥离签名不验证。
///
/// # Errors
///
/// 签名无效或解析失败时返回错误。
pub fn verify_release(
    trust: Option<&TrustStore>,
    inrelease: &[u8],
    now: Timestamp,
) -> AssetsResult<VerifiedRelease> {
    let text = std::str::from_utf8(inrelease)
        .map_err(|_error| fail("bad_release", "InRelease 不是 UTF-8"))?;
    let (body, signer) = match trust {
        Some(store) => {
            let signed = store.verify_cleartext(text, now)?;
            (signed.text, Some(signed.signer))
        }
        None => (
            crate::armor::split_cleartext(text)
                .map_or_else(|_error| text.to_owned(), |parts| parts.text),
            None,
        ),
    };
    let release = Release::parse(&body)?;
    let mut warnings = Vec::new();
    if release.is_stale(now) {
        warnings.push("InRelease 的 Date 早于 30 天，镜像可能滞后".to_owned());
    }
    if signer.is_none() {
        warnings.push("镜像标注为不签名，已跳过签名校验".to_owned());
    }
    Ok(VerifiedRelease {
        release,
        signer,
        warnings,
    })
}

/// 按 `InRelease` 中的条目核对文件内容（哈希与大小）。
///
/// # Errors
///
/// 条目缺失（`not_in_release`）或不符（`hash_mismatch`）时返回错误。
pub fn check_release_entry(release: &Release, path: &str, data: &[u8]) -> AssetsResult<()> {
    let entry = release
        .sha256_of(path)
        .ok_or_else(|| AssetsError::new("not_in_release", format!("InRelease 中没有 {path}")))?;
    let actual = hex::encode(<sha2::Sha256 as sha2::Digest>::digest(data));
    let size = u64::try_from(data.len()).unwrap_or(u64::MAX);
    if entry.sha256 != actual || entry.size != size {
        return Err(AssetsError::new(
            "hash_mismatch",
            format!("{path} 与 InRelease 记录不符"),
        ));
    }
    Ok(())
}

/// 离线（或在线已下载）netboot 的输入。
#[derive(Debug, Clone, Copy)]
pub struct NetbootFiles<'input> {
    /// `InRelease` 内容。
    pub inrelease: &'input [u8],
    /// 安装器 `SHA256SUMS` 内容。
    pub sums: &'input [u8],
    /// 已落盘的内核（内容寻址）。
    pub linux: &'input StoredFile,
    /// 已落盘的 initrd（内容寻址）。
    pub initrd: &'input StoredFile,
}

/// netboot 签名链上的期望哈希。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetbootExpect {
    /// 已验证的 `InRelease`。
    pub release: VerifiedRelease,
    /// 内核期望哈希。
    pub linux: String,
    /// initrd 期望哈希。
    pub initrd: String,
}

/// 签名链第 1–3 步：验证 `InRelease`，核对 `SHA256SUMS`，取出内核与 initrd 的期望哈希。
///
/// # Errors
///
/// 任一环节失败时返回错误。
pub fn netboot_expectations(
    trust: Option<&TrustStore>,
    inrelease: &[u8],
    sums: &[u8],
    now: Timestamp,
) -> AssetsResult<NetbootExpect> {
    let release = verify_release(trust, inrelease, now)?;
    check_release_entry(&release.release, INSTALLER_SUMS, sums)?;
    let text =
        std::str::from_utf8(sums).map_err(|_error| fail("bad_sums", "SHA256SUMS 不是 UTF-8"))?;
    let parsed = Sha256Sums::parse(text)?;
    let lookup = |path: &str| {
        parsed
            .get(path)
            .map(str::to_owned)
            .ok_or_else(|| AssetsError::new("not_in_sums", format!("SHA256SUMS 中没有 {path}")))
    };
    Ok(NetbootExpect {
        linux: lookup(NETBOOT_LINUX)?,
        initrd: lookup(NETBOOT_INITRD)?,
        release,
    })
}

/// 签名链第 3–4 步：核对已落盘文件，读取内核 ABI，写入 netboot 清单并生成待登记资产。
///
/// # Errors
///
/// 哈希不符、内核头不合法或写清单失败时返回错误。
pub async fn finish_netboot(
    dir: &Path,
    expect: &NetbootExpect,
    files: NetbootFiles<'_>,
    origin: (AssetSource, Option<String>),
) -> AssetsResult<VerifiedAsset> {
    for (file, want, name) in [
        (files.linux, &expect.linux, "linux"),
        (files.initrd, &expect.initrd, "initrd.gz"),
    ] {
        if !file.meta.sha256.eq_ignore_ascii_case(want) {
            return Err(AssetsError::new(
                "hash_mismatch",
                format!("{name} 与 SHA256SUMS 记录不符"),
            ));
        }
    }
    let head = read_head(&files.linux.path).await?;
    let kernel = kernel_version(&head)?;
    let manifest = format!(
        "x-pxe netboot manifest v1\nlinux {} {}\ninitrd.gz {} {}\nabi {}\n",
        files.linux.meta.sha256,
        files.linux.meta.size,
        files.initrd.meta.sha256,
        files.initrd.meta.size,
        kernel.abi
    );
    let (sha256, _path) = store_bytes(dir, manifest.as_bytes()).await?;
    let verify = json!({
        "files": {
            "linux": { "sha256": files.linux.meta.sha256, "size": files.linux.meta.size },
            "initrd.gz": { "sha256": files.initrd.meta.sha256, "size": files.initrd.meta.size },
        },
        "signer": expect.release.signer,
        "release_date": expect.release.release.date.map(Timestamp::unix_seconds),
        "kernel_version": kernel.full,
        "warnings": expect.release.warnings,
        "trust": "openpgp",
    });
    Ok(VerifiedAsset {
        kind: AssetKind::DebianNetbootAmd64,
        label: format!("Debian netboot {}", kernel.abi),
        source: origin.0,
        source_url: origin.1,
        sha256,
        size: u64::try_from(manifest.len()).unwrap_or(u64::MAX),
        kernel_abi: Some(kernel.abi),
        verify,
    })
}

/// 读取文件前 [`KERNEL_HEAD`] 字节。
///
/// # Errors
///
/// 读取失败时返回错误。
async fn read_head(path: &Path) -> AssetsResult<Vec<u8>> {
    use tokio::io::AsyncReadExt as _;
    let file = tokio::fs::File::open(path)
        .await
        .map_err(|error| io_error(&error))?;
    let mut head = Vec::with_capacity(KERNEL_HEAD);
    let _read = file
        .take(u64::try_from(KERNEL_HEAD).unwrap_or(u64::MAX))
        .read_to_end(&mut head)
        .await
        .map_err(|error| io_error(&error))?;
    Ok(head)
}

/// 在线获取入口。
#[derive(Debug, Clone, Copy)]
pub struct Ingest<'ctx> {
    /// 下载器。
    pub fetcher: &'ctx Fetcher,
    /// 信任根。
    pub trust: &'ctx TrustStore,
    /// 资产目录。
    pub dir: &'ctx Path,
}

impl Ingest<'_> {
    /// 获取 iPXE：下载到内存、格式检查、内容寻址落盘。
    ///
    /// # Errors
    ///
    /// 下载失败或格式不符时返回错误（不落盘）。
    pub async fn ipxe(self, kind: AssetKind, source: Source<'_>) -> AssetsResult<VerifiedAsset> {
        let (url, origin, asset_source) = match source {
            Source::Official => (
                kind.official_url()
                    .ok_or_else(|| fail("bad_kind", "该种类没有官方地址"))?,
                Origin::Official,
                AssetSource::Official,
            ),
            Source::Url(custom) => (custom, Origin::Url, AssetSource::Url),
        };
        let fetched = self
            .fetcher
            .get_bytes(url, origin, kind.size_limit())
            .await?;
        let format = verify_ipxe(kind, &fetched.data)?;
        let (sha256, _path) = store_bytes(self.dir, &fetched.data).await?;
        let verify = json!({
            "format": format.format,
            "version": format.version,
            "etag": fetched.meta.etag,
            "last_modified": fetched.meta.last_modified,
            "trust": "https+allowlist+manual-activation+canary",
        });
        Ok(VerifiedAsset {
            kind,
            label: format!(
                "{} {}",
                kind.file_name(),
                format.version.as_deref().unwrap_or("（无版本串）")
            ),
            source: asset_source,
            source_url: Some(fetched.meta.final_url),
            sha256,
            size: fetched.meta.size,
            kernel_abi: None,
            verify,
        })
    }

    /// 从镜像源获取 netboot（签名链完整执行）。`signed = false` 表示镜像显式标注不签名。
    ///
    /// # Errors
    ///
    /// 任一环节失败时返回错误；已落盘的成员文件在失败时删除。
    pub async fn netboot(self, mirror: Mirror<'_>, now: Timestamp) -> AssetsResult<VerifiedAsset> {
        let base = mirror.dists();
        let inrelease = self
            .fetcher
            .get_bytes(&format!("{base}/InRelease"), Origin::Mirror, META_LIMIT)
            .await?;
        let sums = self
            .fetcher
            .get_bytes(
                &format!("{base}/{INSTALLER_SUMS}"),
                Origin::Mirror,
                META_LIMIT,
            )
            .await?;
        let trust = mirror.signed.then_some(self.trust);
        let expect = netboot_expectations(trust, &inrelease.data, &sums.data, now)?;
        let images = format!("{base}/main/installer-amd64/current/images");
        let limit = AssetKind::DebianNetbootAmd64.size_limit();
        let linux = self
            .member(
                &format!("{images}/{}", NETBOOT_LINUX.trim_start_matches("./")),
                limit,
                &expect.linux,
            )
            .await?;
        let initrd = match self
            .member(
                &format!("{images}/{}", NETBOOT_INITRD.trim_start_matches("./")),
                limit,
                &expect.initrd,
            )
            .await
        {
            Ok(file) => file,
            Err(error) => {
                remove_file_quietly(&linux.path);
                return Err(error);
            }
        };
        let files = NetbootFiles {
            inrelease: &inrelease.data,
            sums: &sums.data,
            linux: &linux,
            initrd: &initrd,
        };
        let mut asset = finish_netboot(
            self.dir,
            &expect,
            files,
            (AssetSource::Url, Some(mirror.url.to_owned())),
        )
        .await?;
        if let Some(map) = asset.verify.as_object_mut() {
            let _old = map.insert("mirror".to_owned(), Value::from(mirror.url));
            let _suite = map.insert("suite".to_owned(), Value::from(mirror.suite));
        }
        Ok(asset)
    }

    /// 下载一个已知期望哈希的成员文件。
    ///
    /// # Errors
    ///
    /// 下载失败或哈希不符时返回错误。
    async fn member(self, url: &str, limit: u64, expected: &str) -> AssetsResult<StoredFile> {
        let target = FileTarget {
            dir: self.dir,
            limit,
            expected_sha256: Some(expected),
        };
        self.fetcher.get_file(url, Origin::Mirror, target).await
    }

    /// 镜像对齐检查：验证镜像 `InRelease`，按其哈希下载 `Packages.xz`（无则 `.gz`），
    /// 确认存在 `kernel-image-<ABI>-di`。
    ///
    /// # Errors
    ///
    /// 签名无效、两种 `Packages` 都不在 `InRelease` 中、下载或解压失败时返回错误。
    pub async fn alignment(
        self,
        mirror: Mirror<'_>,
        abi: &str,
        now: Timestamp,
    ) -> AssetsResult<AlignmentReport> {
        let base = mirror.dists();
        let inrelease = self
            .fetcher
            .get_bytes(&format!("{base}/InRelease"), Origin::Mirror, META_LIMIT)
            .await?;
        let release = verify_release(mirror.signed.then_some(self.trust), &inrelease.data, now)?;
        let candidates = [
            (PACKAGES_XZ, Compression::Xz),
            (PACKAGES_GZ, Compression::Gzip),
        ];
        let (path, compression) = candidates
            .into_iter()
            .find(|(path, _)| release.release.sha256_of(path).is_some())
            .ok_or_else(|| {
                fail(
                    "not_in_release",
                    "InRelease 中没有 debian-installer 的 Packages",
                )
            })?;
        let packages = self
            .fetcher
            .get_bytes(&format!("{base}/{path}"), Origin::Mirror, META_LIMIT)
            .await?;
        check_release_entry(&release.release, path, &packages.data)?;
        let text = decompress_packages(&packages.data, compression)?;
        Ok(AlignmentReport {
            alignment: check_alignment(abi, &text),
            mirror: mirror.url.to_owned(),
            packages_path: path.to_owned(),
            signer: release.signer,
            release_date: release.release.date.map(Timestamp::unix_seconds),
            checked_at: now.unix_seconds(),
        })
    }

    /// 获取固件 `firmware.cpio.gz`：`SHA256SUMS.sign` 验证 `SHA256SUMS`，再比对文件哈希。
    ///
    /// # Errors
    ///
    /// 签名无效、条目缺失或哈希不符时返回错误。
    pub async fn firmware(self, source: Source<'_>, now: Timestamp) -> AssetsResult<VerifiedAsset> {
        let kind = AssetKind::DebianFirmwareCpio;
        let (raw_base, origin, asset_source) = match source {
            Source::Official => (
                kind.official_url().unwrap_or_default(),
                Origin::Official,
                AssetSource::Official,
            ),
            Source::Url(custom) => (custom, Origin::Url, AssetSource::Url),
        };
        let base = raw_base.trim_end_matches('/');
        let sums = self
            .fetcher
            .get_bytes(&format!("{base}/SHA256SUMS"), origin, META_LIMIT)
            .await?;
        let sign = self
            .fetcher
            .get_bytes(&format!("{base}/SHA256SUMS.sign"), origin, META_LIMIT)
            .await?;
        let signer = self.trust.verify_detached(&sums.data, &sign.data, now)?;
        let text = std::str::from_utf8(&sums.data)
            .map_err(|_error| fail("bad_sums", "SHA256SUMS 不是 UTF-8"))?;
        let want = Sha256Sums::parse(text)?
            .get(kind.file_name())
            .map(str::to_owned)
            .ok_or_else(|| fail("not_in_sums", "SHA256SUMS 中没有 firmware.cpio.gz"))?;
        let target = FileTarget {
            dir: self.dir,
            limit: kind.size_limit(),
            expected_sha256: Some(&want),
        };
        let stored = self
            .fetcher
            .get_file(&format!("{base}/{}", kind.file_name()), origin, target)
            .await?;
        Ok(VerifiedAsset {
            kind,
            label: "Debian firmware.cpio.gz".to_owned(),
            source: asset_source,
            source_url: Some(stored.meta.final_url),
            sha256: stored.meta.sha256,
            size: stored.meta.size,
            kernel_abi: None,
            verify: json!({ "signer": signer, "etag": stored.meta.etag, "trust": "openpgp-detached" }),
        })
    }
}

/// 尽力删除文件。
fn remove_file_quietly(path: &Path) {
    let _removed: Result<(), std::io::Error> = std::fs::remove_file(path);
}
