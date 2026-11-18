//! 管理端 HTTPS：从 PEM 文件加载证书链与私钥；任一不可用即启动失败。

use std::path::Path;
use std::sync::Arc;

use rustls::ServerConfig;
use rustls::pki_types::pem::PemObject as _;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};

/// 构造 rustls 服务端配置（只开 HTTP/1.1）。
///
/// # Errors
///
/// 证书或私钥读取、解析失败，或二者不匹配时返回错误说明。
pub(crate) fn server_config(cert: &Path, key: &Path) -> Result<Arc<ServerConfig>, String> {
    let chain = CertificateDer::pem_file_iter(cert)
        .and_then(Iterator::collect::<Result<Vec<_>, _>>)
        .map_err(|error| format!("读取证书 {} 失败：{error}", cert.display()))?;
    if chain.is_empty() {
        return Err(format!("证书文件 {} 不含证书", cert.display()));
    }
    let private = PrivateKeyDer::from_pem_file(key)
        .map_err(|error| format!("读取私钥 {} 失败：{error}", key.display()))?;
    let mut config =
        ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .map_err(|error| error.to_string())?
            .with_no_client_auth()
            .with_single_cert(chain, private)
            .map_err(|error| format!("证书与私钥不可用：{error}"))?;
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(Arc::new(config))
}
