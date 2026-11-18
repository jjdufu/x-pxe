//! 受控下载：主机白名单、重定向上限、大小上限、流式 SHA-256 与内容寻址落盘。

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use http::{HeaderMap, Request, StatusCode, Uri, header};
use http_body_util::{BodyExt as _, Empty};
use hyper::body::Incoming;
use hyper_rustls::HttpsConnector;
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::TokioExecutor;
use rustls::pki_types::CertificateDer;
use rustls::pki_types::pem::PemObject as _;
use sha2::{Digest as _, Sha256};
use tokio::io::AsyncWriteExt as _;

use crate::error::{AssetsError, AssetsResult, fail};

/// 官方资产主机白名单（含 GitHub 下载重定向主机）。
pub const OFFICIAL_HOSTS: [&str; 6] = [
    "boot.ipxe.org",
    "github.com",
    "objects.githubusercontent.com",
    "release-assets.githubusercontent.com",
    "deb.debian.org",
    "cdimage.debian.org",
];
/// 最多跟随的重定向次数。
pub const MAX_REDIRECTS: usize = 5;

/// 下载来源类别，决定允许的协议与主机。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// 官方源：仅 HTTPS，且主机（含重定向目标）必须在白名单内；离线模式禁止。
    Official,
    /// 管理员填写的任意 HTTPS URL（如内网制品库）；离线模式下不得指向官方公网主机。
    Url,
    /// Profile 中的镜像源：HTTP 或 HTTPS，离线模式照常访问。
    Mirror,
}

/// 下载策略。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchPolicy {
    /// 离线模式（`XPXE_OFFLINE=true`）。
    pub offline: bool,
    /// 官方主机白名单。
    pub official_hosts: Vec<String>,
    /// 等待响应头的超时。
    pub header_timeout: Duration,
    /// 两个数据帧之间的最长间隔。
    pub idle_timeout: Duration,
}

impl FetchPolicy {
    /// 生产默认值。
    #[must_use]
    pub fn standard(offline: bool) -> Self {
        Self {
            offline,
            official_hosts: OFFICIAL_HOSTS
                .iter()
                .map(|host| (*host).to_owned())
                .collect(),
            header_timeout: Duration::from_secs(30),
            idle_timeout: Duration::from_secs(60),
        }
    }

    /// 检查 URL 是否允许访问。
    ///
    /// # Errors
    ///
    /// 协议或主机不被允许时返回 `host_denied`。
    pub fn check(&self, uri: &Uri, origin: Origin) -> AssetsResult<()> {
        let host = uri.host().unwrap_or_default().to_ascii_lowercase();
        let scheme = uri.scheme_str().unwrap_or_default();
        let official = self.official_hosts.contains(&host);
        let denied = |reason: &str| AssetsError::new("host_denied", format!("{uri}：{reason}"));
        match origin {
            Origin::Official if self.offline => Err(denied("离线模式禁止访问官方公网源")),
            Origin::Official if scheme != "https" => Err(denied("官方源只允许 HTTPS")),
            Origin::Official if !official => Err(denied("主机不在官方白名单内")),
            Origin::Url if scheme != "https" => Err(denied("自定义来源只允许 HTTPS")),
            Origin::Mirror if scheme != "https" && scheme != "http" => {
                Err(denied("镜像源只允许 HTTP/HTTPS"))
            }
            Origin::Url | Origin::Mirror if self.offline && official => {
                Err(denied("离线模式禁止访问官方公网源"))
            }
            Origin::Official | Origin::Url | Origin::Mirror if host.is_empty() => {
                Err(denied("缺少主机名"))
            }
            Origin::Official | Origin::Url | Origin::Mirror => Ok(()),
        }
    }
}

/// 下载元数据。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct FetchMeta {
    /// 重定向后的最终 URL。
    pub final_url: String,
    /// HTTP `ETag`。
    pub etag: Option<String>,
    /// HTTP `Last-Modified`。
    pub last_modified: Option<String>,
    /// 内容 SHA-256（十六进制小写）。
    pub sha256: String,
    /// 字节数。
    pub size: u64,
}

/// 读入内存的下载结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fetched {
    /// 元数据。
    pub meta: FetchMeta,
    /// 内容。
    pub data: Vec<u8>,
}

/// 落盘到内容寻址路径的下载结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredFile {
    /// 元数据。
    pub meta: FetchMeta,
    /// `<目录>/<sha256>`。
    pub path: PathBuf,
}

/// 落盘目标。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileTarget<'path> {
    /// 内容寻址目录（`/data/assets`）。
    pub dir: &'path Path,
    /// 大小上限。
    pub limit: u64,
    /// 期望的 SHA-256；不符时删除临时文件并返回 `hash_mismatch`。
    pub expected_sha256: Option<&'path str>,
}

/// 写入目标。
enum Sink {
    /// 内存。
    Memory(Vec<u8>),
    /// 临时文件。
    File(tokio::fs::File),
}

impl Sink {
    /// 追加数据。
    ///
    /// # Errors
    ///
    /// 写文件失败时返回错误。
    async fn write(&mut self, chunk: &[u8]) -> AssetsResult<()> {
        match self {
            Self::Memory(buffer) => {
                buffer.extend_from_slice(chunk);
                Ok(())
            }
            Self::File(file) => file
                .write_all(chunk)
                .await
                .map_err(|error| io_error(&error)),
        }
    }
}

/// HTTP(S) 下载器。
#[derive(Debug, Clone)]
pub struct Fetcher {
    /// hyper 客户端。
    client: Client<HttpsConnector<HttpConnector>, Empty<Bytes>>,
    /// 策略。
    policy: FetchPolicy,
}

impl Fetcher {
    /// 使用 webpki 内置根证书。
    #[must_use]
    pub fn new(policy: FetchPolicy) -> Self {
        let mut roots = rustls::RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        Self::with_roots(policy, &roots)
    }

    /// 仅信任给定 PEM 中的根证书（测试与内网 CA）。
    ///
    /// # Errors
    ///
    /// PEM 无法解析或其中没有证书时返回错误。
    pub fn with_root_pem(policy: FetchPolicy, pem: &[u8]) -> AssetsResult<Self> {
        let mut roots = rustls::RootCertStore::empty();
        for cert in CertificateDer::pem_slice_iter(pem) {
            let der = cert
                .map_err(|error| AssetsError::new("bad_ca", format!("CA PEM 解析失败：{error}")))?;
            roots
                .add(der)
                .map_err(|error| AssetsError::new("bad_ca", format!("CA 证书不合法：{error}")))?;
        }
        if roots.is_empty() {
            return Err(fail("bad_ca", "PEM 中没有证书"));
        }
        Ok(Self::with_roots(policy, &roots))
    }

    /// 由根证书集合构造。
    fn with_roots(policy: FetchPolicy, roots: &rustls::RootCertStore) -> Self {
        let config = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .map_or_else(
            |_error| rustls::ClientConfig::builder().with_root_certificates(roots.clone()),
            |builder| builder.with_root_certificates(roots.clone()),
        )
        .with_no_client_auth();
        let connector = hyper_rustls::HttpsConnectorBuilder::new()
            .with_tls_config(config)
            .https_or_http()
            .enable_http1()
            .build();
        Self {
            client: Client::builder(TokioExecutor::new()).build(connector),
            policy,
        }
    }

    /// 策略。
    #[must_use]
    pub const fn policy(&self) -> &FetchPolicy {
        &self.policy
    }

    /// 下载到内存（小文件：`InRelease`、`SHA256SUMS`、`Packages.xz`）。
    ///
    /// # Errors
    ///
    /// 策略拒绝、HTTP 错误、超限、断流或超时时返回错误。
    pub async fn get_bytes(&self, url: &str, origin: Origin, limit: u64) -> AssetsResult<Fetched> {
        let (response, final_url) = self.open(url, origin).await?;
        let headers = response.headers().clone();
        let mut sink = Sink::Memory(Vec::new());
        let (sha256, size) = self.drain(response, limit, &mut sink).await?;
        let data = match sink {
            Sink::Memory(buffer) => buffer,
            Sink::File(_) => Vec::new(),
        };
        Ok(Fetched {
            meta: meta(final_url, &headers, sha256, size),
            data,
        })
    }

    /// 流式下载到 `<dir>/.tmp/<随机名>`，校验通过后改名为 `<dir>/<sha256>`；失败时删除临时文件。
    ///
    /// # Errors
    ///
    /// 策略拒绝、HTTP 错误、超限、断流、超时、哈希不符或文件系统错误时返回错误。
    pub async fn get_file(
        &self,
        url: &str,
        origin: Origin,
        target: FileTarget<'_>,
    ) -> AssetsResult<StoredFile> {
        let (response, final_url) = self.open(url, origin).await?;
        let headers = response.headers().clone();
        let tmp = temp_path(target.dir).await?;
        let file = tokio::fs::File::create(&tmp)
            .await
            .map_err(|error| io_error(&error))?;
        let mut sink = Sink::File(file);
        let outcome = self.drain(response, target.limit, &mut sink).await;
        let finished = match (outcome, sink) {
            (Ok(done), Sink::File(written)) => written
                .sync_all()
                .await
                .map(|()| done)
                .map_err(|error| io_error(&error)),
            (Ok(done), Sink::Memory(_)) => Ok(done),
            (Err(error), _) => Err(error),
        }
        .and_then(|(sha256, size)| check_expected(target.expected_sha256, sha256, size));
        let (sha256, size) = match finished {
            Ok(done) => done,
            Err(error) => {
                remove_quietly(&tmp).await;
                return Err(error);
            }
        };
        let path = target.dir.join(&sha256);
        if let Err(error) = tokio::fs::rename(&tmp, &path).await {
            remove_quietly(&tmp).await;
            return Err(io_error(&error));
        }
        Ok(StoredFile {
            meta: meta(final_url, &headers, sha256, size),
            path,
        })
    }

    /// 发起 GET 并跟随重定向（每一跳都重新检查策略）。
    ///
    /// # Errors
    ///
    /// 策略拒绝、重定向过多、超时或非 2xx 状态时返回错误。
    async fn open(
        &self,
        url: &str,
        origin: Origin,
    ) -> AssetsResult<(hyper::Response<Incoming>, String)> {
        let mut current: Uri = url
            .parse()
            .map_err(|_error| AssetsError::new("bad_url", format!("URL 不合法：{url}")))?;
        for _hop in 0..=MAX_REDIRECTS {
            self.policy.check(&current, origin)?;
            let request = Request::get(current.clone())
                .header(header::USER_AGENT, "x-pxe-assets")
                .body(Empty::new())
                .map_err(|error| AssetsError::new("bad_url", error.to_string()))?;
            let response =
                tokio::time::timeout(self.policy.header_timeout, self.client.request(request))
                    .await
                    .map_err(|_elapsed| {
                        AssetsError::new("timeout", format!("{current}：等待响应超时"))
                    })?
                    .map_err(|error| {
                        AssetsError::new("http_error", format!("{current}：{error}"))
                    })?;
            let status = response.status();
            if status.is_success() {
                return Ok((response, current.to_string()));
            }
            if !is_redirect(status) {
                return Err(AssetsError::new(
                    "http_status",
                    format!("{current}：HTTP {}", status.as_u16()),
                ));
            }
            current = redirect_target(&current, response.headers())?;
        }
        Err(AssetsError::new(
            "too_many_redirects",
            format!("重定向超过 {MAX_REDIRECTS} 次"),
        ))
    }

    /// 读取响应体写入 `sink`，边读边算 SHA-256；超过 `limit` 立即中止。
    ///
    /// # Errors
    ///
    /// 超限、断流、帧间超时或写入失败时返回错误。
    async fn drain(
        &self,
        response: hyper::Response<Incoming>,
        limit: u64,
        sink: &mut Sink,
    ) -> AssetsResult<(String, u64)> {
        let declared = response
            .headers()
            .get(header::CONTENT_LENGTH)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok());
        if declared.is_some_and(|len| len > limit) {
            return Err(too_large(limit));
        }
        let mut body = response.into_body();
        let mut hasher = Sha256::new();
        let mut size = 0_u64;
        loop {
            let next = tokio::time::timeout(self.policy.idle_timeout, body.frame())
                .await
                .map_err(|_elapsed| fail("timeout", "下载中断：数据帧超时"))?;
            let Some(frame) = next else {
                break;
            };
            let data_frame = frame
                .map_err(|error| AssetsError::new("stream_broken", format!("下载中断：{error}")))?;
            let Ok(chunk) = data_frame.into_data() else {
                continue;
            };
            size = size.saturating_add(u64::try_from(chunk.len()).unwrap_or(u64::MAX));
            if size > limit {
                return Err(too_large(limit));
            }
            hasher.update(&chunk);
            sink.write(&chunk).await?;
        }
        if declared.is_some_and(|len| len != size) {
            return Err(fail(
                "stream_broken",
                "下载中断：长度与 Content-Length 不符",
            ));
        }
        Ok((hex::encode(hasher.finalize()), size))
    }
}

/// 比对期望哈希。
///
/// # Errors
///
/// 不符时返回 `hash_mismatch`。
pub(crate) fn check_expected(
    expected: Option<&str>,
    sha256: String,
    size: u64,
) -> AssetsResult<(String, u64)> {
    match expected {
        Some(want) if !want.eq_ignore_ascii_case(&sha256) => Err(AssetsError::new(
            "hash_mismatch",
            format!("SHA-256 不符：期望 {want}，实际 {sha256}"),
        )),
        Some(_) | None => Ok((sha256, size)),
    }
}

/// 在 `<dir>/.tmp/` 下生成一个临时文件路径（目录不存在时创建）。
///
/// # Errors
///
/// 创建目录或随机源失败时返回错误。
async fn temp_path(dir: &Path) -> AssetsResult<PathBuf> {
    let tmp_dir = dir.join(".tmp");
    tokio::fs::create_dir_all(&tmp_dir)
        .await
        .map_err(|error| io_error(&error))?;
    Ok(tmp_dir.join(random_name()?))
}

/// 把内存中的内容以内容寻址方式落盘（先写临时文件再改名），返回最终路径。
///
/// # Errors
///
/// 文件系统错误时返回错误。
pub async fn store_bytes(dir: &Path, data: &[u8]) -> AssetsResult<(String, PathBuf)> {
    let sha256 = hex::encode(Sha256::digest(data));
    let tmp = temp_path(dir).await?;
    let written = async {
        let mut file = tokio::fs::File::create(&tmp).await?;
        file.write_all(data).await?;
        file.sync_all().await?;
        tokio::fs::rename(&tmp, dir.join(&sha256)).await
    }
    .await;
    if let Err(error) = written {
        remove_quietly(&tmp).await;
        return Err(io_error(&error));
    }
    let path = dir.join(&sha256);
    Ok((sha256, path))
}

/// 是否为重定向状态码。
const fn is_redirect(status: StatusCode) -> bool {
    matches!(status.as_u16(), 301 | 302 | 303 | 307 | 308)
}

/// 解析 `Location`：绝对 URL 或以 `/` 开头的路径。
///
/// # Errors
///
/// 缺少或不支持的 `Location` 时返回错误。
fn redirect_target(current: &Uri, headers: &HeaderMap) -> AssetsResult<Uri> {
    let location = headers
        .get(header::LOCATION)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| fail("bad_redirect", "重定向缺少 Location"))?;
    let absolute = if location.starts_with("https://") || location.starts_with("http://") {
        location.to_owned()
    } else if location.starts_with('/') && !location.starts_with("//") {
        let scheme = current.scheme_str().unwrap_or("https");
        let authority = current.authority().map_or("", http::uri::Authority::as_str);
        format!("{scheme}://{authority}{location}")
    } else {
        return Err(AssetsError::new(
            "bad_redirect",
            format!("不支持的 Location：{location}"),
        ));
    };
    absolute
        .parse()
        .map_err(|_error| AssetsError::new("bad_redirect", format!("Location 不合法：{location}")))
}

/// 组装元数据。
fn meta(final_url: String, headers: &HeaderMap, sha256: String, size: u64) -> FetchMeta {
    let text = |name: header::HeaderName| {
        headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned)
    };
    FetchMeta {
        final_url,
        etag: text(header::ETAG),
        last_modified: text(header::LAST_MODIFIED),
        sha256,
        size,
    }
}

/// 超限错误。
fn too_large(limit: u64) -> AssetsError {
    AssetsError::new("too_large", format!("超过大小上限 {limit} 字节，已中止"))
}

/// I/O 错误。
pub(crate) fn io_error(error: &std::io::Error) -> AssetsError {
    AssetsError::new("io", error.to_string())
}

/// 随机临时文件名。
///
/// # Errors
///
/// 系统随机源失败时返回错误。
pub(crate) fn random_name() -> AssetsResult<String> {
    let mut bytes = [0_u8; 16];
    ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut bytes)
        .map_err(|_error| fail("random", "系统随机源不可用"))?;
    Ok(hex::encode(bytes))
}

/// 尽力删除临时文件（失败无影响：`.tmp` 在启动时整体清理）。
async fn remove_quietly(path: &Path) {
    let _removed: Result<(), std::io::Error> = tokio::fs::remove_file(path).await;
}

#[cfg(test)]
mod tests;
