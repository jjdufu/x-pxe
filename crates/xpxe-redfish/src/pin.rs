//! 证书指纹与自定义 `ServerCertVerifier`：固定模式只认终端证书 DER 的 SHA-256，探测模式接受任意证书并记录。

use std::sync::{Arc, Mutex};

use rustls::DigitallySignedStruct;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{WebPkiSupportedAlgorithms, verify_tls12_signature, verify_tls13_signature};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use sha2::{Digest as _, Sha256};

use crate::{RedfishError, RedfishResult};

/// 终端证书 DER 的 SHA-256 指纹。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Fingerprint([u8; 32]);

impl Fingerprint {
    /// 计算 DER 的指纹。
    #[must_use]
    pub fn of_der(der: &[u8]) -> Self {
        Self(Sha256::digest(der).into())
    }

    /// 解析 64 位十六进制（允许 `:` 分隔、大小写不敏感，可带 `sha256:` 前缀）。
    ///
    /// # Errors
    ///
    /// 长度或字符不合法时返回 `bad_fingerprint`。
    pub fn parse(raw: &str) -> RedfishResult<Self> {
        let text: String = raw
            .trim()
            .trim_start_matches("sha256:")
            .chars()
            .filter(|ch| *ch != ':')
            .collect();
        let mut out = [0_u8; 32];
        hex::decode_to_slice(&text, &mut out).map_err(|_error| {
            RedfishError::new("bad_fingerprint", "指纹应为 64 位十六进制".to_owned())
        })?;
        Ok(Self(out))
    }

    /// 原始字节。
    #[must_use]
    pub const fn bytes(&self) -> [u8; 32] {
        self.0
    }
}

impl std::fmt::Display for Fingerprint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&hex::encode(self.0))
    }
}

/// 握手中看到的终端证书（DER）。
pub(crate) type SeenSlot = Arc<Mutex<Option<Vec<u8>>>>;

/// BMC 证书校验器。
#[derive(Debug)]
pub(crate) struct BmcVerifier {
    /// `Some` 为固定模式；`None` 为探测模式（接受任意证书）。
    pin: Option<Fingerprint>,
    /// 最近一次握手的终端证书。
    seen: SeenSlot,
    /// 握手签名算法（rustls 标准校验）。
    algorithms: WebPkiSupportedAlgorithms,
}

impl BmcVerifier {
    /// 构造校验器。
    pub(crate) fn new(pin: Option<Fingerprint>, seen: SeenSlot) -> Self {
        Self {
            pin,
            seen,
            algorithms: rustls::crypto::ring::default_provider().signature_verification_algorithms,
        }
    }
}

impl ServerCertVerifier for BmcVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        let der = end_entity.as_ref().to_vec();
        let actual = Fingerprint::of_der(&der);
        if let Ok(mut slot) = self.seen.lock() {
            *slot = Some(der);
        }
        match self.pin {
            Some(expected) if expected != actual => Err(rustls::Error::InvalidCertificate(
                rustls::CertificateError::ApplicationVerificationFailure,
            )),
            Some(_) | None => Ok(ServerCertVerified::assertion()),
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(message, cert, dss, &self.algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(message, cert, dss, &self.algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.algorithms.supported_schemes()
    }
}

#[cfg(test)]
mod tests;
