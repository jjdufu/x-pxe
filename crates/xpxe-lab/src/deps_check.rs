//! 依赖预检：每个依赖调用一次最小 API。

use std::io::{Read as _, Write as _};
use std::sync::Arc;

use base64::Engine as _;
use chacha20poly1305::aead::{Aead as _, KeyInit as _};
use password_hash_reexport::PasswordHasher as _;
use ring::rand::SecureRandom as _;
use sha2::Digest as _;

/// argon2 与 sha-crypt 共用的 `PasswordHasher` trait。
mod password_hash_reexport {
    pub(crate) use argon2::PasswordHasher;
}

/// 预检失败的原因。
#[derive(Debug)]
pub(crate) enum CheckError {
    /// 命令行用法错误。
    Usage,
    /// 某个依赖的最小调用失败。
    Dependency {
        /// 依赖名。
        name: &'static str,
        /// 失败详情。
        detail: String,
    },
}

impl std::fmt::Display for CheckError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Usage => f.write_str("用法：xpxe-lab deps-check"),
            Self::Dependency { name, detail } => write!(f, "依赖 {name} 预检失败：{detail}"),
        }
    }
}

impl std::error::Error for CheckError {}

/// 把任意错误包装成某个依赖的预检失败。
fn fail(name: &'static str) -> impl FnOnce(&dyn std::fmt::Display) -> CheckError {
    move |error| CheckError::Dependency {
        name,
        detail: error.to_string(),
    }
}

/// 条件不成立时报告依赖失败。
///
/// # Errors
///
/// `cond` 为假时返回 [`CheckError::Dependency`]。
fn require(cond: bool, name: &'static str, detail: &str) -> Result<(), CheckError> {
    if cond {
        Ok(())
    } else {
        Err(CheckError::Dependency {
            name,
            detail: detail.to_owned(),
        })
    }
}

/// 单项预检函数。
type Check = fn() -> Result<(), CheckError>;

/// 健康检查路由的处理函数。
async fn healthz() -> &'static str {
    "ok"
}

/// 依次执行全部预检，返回通过的依赖名。
///
/// # Errors
///
/// 任一依赖失败即返回对应错误。
pub(crate) fn run() -> Result<Vec<&'static str>, CheckError> {
    let checks: [(&'static str, Check); 9] = [
        ("tokio+axum+tower-http+hyper", check_http_stack),
        (
            "rustls+tokio-rustls+hyper-rustls+webpki-roots+ring",
            check_tls,
        ),
        ("socket2", check_socket2),
        ("minijinja", check_minijinja),
        ("sha2+hex+base64", check_encoding),
        ("lzma-rs+flate2", check_compression),
        ("rusqlite", check_sqlite),
        ("argon2+chacha20poly1305+sha-crypt", check_crypto),
        ("serde+serde_json+time+tracing", check_serde_time_tracing),
    ];
    let mut passed = Vec::with_capacity(checks.len());
    for (name, check) in checks {
        check()?;
        passed.push(name);
    }
    Ok(passed)
}

/// tokio 运行时、axum 路由、tower-http 层、hyper 与 http-body-util。
///
/// # Errors
///
/// 运行时或路由调用失败时返回错误。
fn check_http_stack() -> Result<(), CheckError> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| fail("tokio")(&error))?;
    let router: axum::Router = axum::Router::new()
        .route("/healthz", axum::routing::get(healthz))
        .layer(tower_http::limit::RequestBodyLimitLayer::new(1024));
    let body = http_body_util::Full::new(bytes::Bytes::from_static(b"ping"));
    let request = http::Request::builder()
        .uri("/healthz")
        .body(body)
        .map_err(|error| fail("http")(&error))?;
    let builder = hyper::server::conn::http1::Builder::new();
    let executor = hyper_util::rt::TokioExecutor::new();
    require(
        !format!("{builder:?}{executor:?}").is_empty(),
        "hyper",
        "hyper 构建器不可用",
    )?;
    let outcome = runtime.block_on(async move {
        use tower::ServiceExt as _;
        router
            .oneshot(request)
            .await
            .map(|response| response.status().as_u16())
    });
    let code = outcome.map_err(|error| fail("axum")(&error))?;
    require(code == 200_u16, "axum", "路由必须返回 200")
}

/// rustls（ring provider）、tokio-rustls、hyper-rustls 与 webpki 根证书。
///
/// # Errors
///
/// TLS 配置构建失败时返回错误。
fn check_tls() -> Result<(), CheckError> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    require(!roots.is_empty(), "webpki-roots", "根证书不能为空")?;
    let built = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|error| fail("rustls")(&error))?
        .with_root_certificates(roots)
        .with_no_client_auth();
    let config = Arc::new(built);
    let _connector: tokio_rustls::TlsConnector =
        tokio_rustls::TlsConnector::from(Arc::clone(&config));
    let _https: hyper_rustls::HttpsConnector<hyper_util::client::legacy::connect::HttpConnector> =
        hyper_rustls::HttpsConnectorBuilder::new()
            .with_tls_config(rustls::ClientConfig::clone(&config))
            .https_or_http()
            .enable_http1()
            .build();
    let mut random = [0_u8; 32];
    ring::rand::SystemRandom::new()
        .fill(&mut random)
        .map_err(|error| fail("ring")(&error))?;
    require(
        random.iter().any(|byte| *byte != 0),
        "ring",
        "随机数不应全零",
    )
}

/// UDP 广播套接字。
///
/// # Errors
///
/// 套接字创建或选项设置失败时返回错误。
fn check_socket2() -> Result<(), CheckError> {
    let socket = socket2::Socket::new(socket2::Domain::IPV4, socket2::Type::DGRAM, None)
        .map_err(|error| fail("socket2")(&error))?;
    socket
        .set_broadcast(true)
        .map_err(|error| fail("socket2")(&error))?;
    let broadcast = socket
        .broadcast()
        .map_err(|error| fail("socket2")(&error))?;
    require(broadcast, "socket2", "SO_BROADCAST 必须生效")
}

/// 模板渲染。
///
/// # Errors
///
/// 模板编译或渲染失败时返回错误。
fn check_minijinja() -> Result<(), CheckError> {
    let mut env = minijinja::Environment::new();
    env.add_template("t", "chain {{ url }}")
        .map_err(|error| fail("minijinja")(&error))?;
    let template = env
        .get_template("t")
        .map_err(|error| fail("minijinja")(&error))?;
    let rendered = template
        .render(minijinja::context! { url => "http://x:8080/boot.ipxe" })
        .map_err(|error| fail("minijinja")(&error))?;
    require(
        rendered == "chain http://x:8080/boot.ipxe",
        "minijinja",
        "渲染结果不符",
    )
}

/// SHA-256、hex 与 base64url。
///
/// # Errors
///
/// 结果与已知值不符时返回错误。
fn check_encoding() -> Result<(), CheckError> {
    let digest = sha2::Sha256::digest(b"abc");
    let hexed = hex::encode(digest);
    require(
        hexed == "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        "sha2",
        "SHA-256(abc) 不符",
    )?;
    let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b"x-pxe");
    require(encoded == "eC1weGU", "base64", "base64url 编码不符")
}

/// xz 与 gzip 往返。
///
/// # Errors
///
/// 压缩或解压失败、往返不一致时返回错误。
fn check_compression() -> Result<(), CheckError> {
    let input = b"Package: xpxe\n".repeat(8);
    let mut xz = Vec::new();
    lzma_rs::xz_compress(&mut input.as_slice(), &mut xz)
        .map_err(|error| fail("lzma-rs")(&error))?;
    let mut plain = Vec::new();
    lzma_rs::xz_decompress(&mut xz.as_slice(), &mut plain)
        .map_err(|error| fail("lzma-rs")(&error))?;
    require(plain == input, "lzma-rs", "xz 往返不一致")?;
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder
        .write_all(&input)
        .map_err(|error| fail("flate2")(&error))?;
    let gz = encoder.finish().map_err(|error| fail("flate2")(&error))?;
    let mut unzipped = Vec::new();
    let _read: usize = flate2::read::GzDecoder::new(gz.as_slice())
        .read_to_end(&mut unzipped)
        .map_err(|error| fail("flate2")(&error))?;
    require(unzipped == input, "flate2", "gzip 往返不一致")
}

/// 内存 SQLite 与内置版本。
///
/// # Errors
///
/// 打开或查询失败时返回错误。
fn check_sqlite() -> Result<(), CheckError> {
    let conn = rusqlite::Connection::open_in_memory().map_err(|error| fail("rusqlite")(&error))?;
    let mut stmt = conn
        .prepare("SELECT 40 + 2")
        .map_err(|error| fail("rusqlite")(&error))?;
    let mut rows = stmt.query([]).map_err(|error| fail("rusqlite")(&error))?;
    let row = rows
        .next()
        .map_err(|error| fail("rusqlite")(&error))?
        .ok_or_else(|| fail("rusqlite")(&"查询没有返回行"))?;
    let value: i64 = row.get(0).map_err(|error| fail("rusqlite")(&error))?;
    require(value == 42_i64, "rusqlite", "SELECT 40 + 2 应为 42")
}

/// argon2id 口令哈希、XChaCha20-Poly1305 封装、`$6$` 哈希。
///
/// # Errors
///
/// 任一算法调用失败或往返不一致时返回错误。
fn check_crypto() -> Result<(), CheckError> {
    let rng = ring::rand::SystemRandom::new();
    let mut salt = [0_u8; 16];
    rng.fill(&mut salt).map_err(|error| fail("ring")(&error))?;
    let hash = argon2::Argon2::default()
        .hash_password_with_salt(b"pw", &salt)
        .map_err(|error| fail("argon2")(&error))?;
    require(
        hash.to_string().starts_with("$argon2id$"),
        "argon2",
        "必须是 argon2id",
    )?;

    let mut key = [0_u8; 32];
    let mut nonce = [0_u8; 24];
    rng.fill(&mut key).map_err(|error| fail("ring")(&error))?;
    rng.fill(&mut nonce).map_err(|error| fail("ring")(&error))?;
    let cipher = chacha20poly1305::XChaCha20Poly1305::new(&key.into());
    let sealed = cipher
        .encrypt(&nonce.into(), b"bmc-secret".as_slice())
        .map_err(|error| fail("chacha20poly1305")(&error))?;
    let opened = cipher
        .decrypt(&nonce.into(), sealed.as_slice())
        .map_err(|error| fail("chacha20poly1305")(&error))?;
    require(
        opened == b"bmc-secret",
        "chacha20poly1305",
        "封装往返不一致",
    )?;

    let crypt = sha_crypt::ShaCrypt::default()
        .hash_password_with_salt(b"pw", b"saltsalt")
        .map_err(|error| fail("sha-crypt")(&error))?;
    require(
        crypt.as_str().starts_with("$6$"),
        "sha-crypt",
        "必须是 $6$ 哈希",
    )
}

/// 预检用的序列化样本。
#[derive(Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
struct Sample {
    /// 名称。
    name: String,
}

/// serde 往返、RFC 3339 时间格式化、tracing 订阅器。
///
/// # Errors
///
/// 任一调用失败时返回错误。
fn check_serde_time_tracing() -> Result<(), CheckError> {
    let sample = Sample {
        name: "node-01".to_owned(),
    };
    let json = serde_json::to_string(&sample).map_err(|error| fail("serde_json")(&error))?;
    let back: Sample = serde_json::from_str(&json).map_err(|error| fail("serde_json")(&error))?;
    require(back == sample, "serde", "JSON 往返不一致")?;
    let now = time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .map_err(|error| fail("time")(&error))?;
    require(now.ends_with('Z'), "time", "UTC 时间应以 Z 结尾")?;
    let subscriber = tracing_subscriber::fmt()
        .json()
        .with_env_filter(tracing_subscriber::EnvFilter::new("info"))
        .with_writer(std::io::sink)
        .finish();
    tracing::subscriber::with_default(
        subscriber,
        || tracing::info!(target: "deps_check", "tracing ok"),
    );
    Ok(())
}
