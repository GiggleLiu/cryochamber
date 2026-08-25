//! What the app can find out about a hub's certificate before a token is ever
//! sent to it. The WebView cannot: a browser answers a bad certificate with an
//! opaque network error and never says *which* certificate it saw. Rust can, so
//! the shell does the looking and the console does the asking.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};
use sha2::{Digest, Sha256};

/// What one look at a hub's TLS says.
#[derive(serde::Serialize)]
pub struct ProbeReport {
    /// TLS handshake succeeded against the system trust store.
    pub https_valid: bool,
    /// SHA-256 of the end-entity certificate DER, lowercase hex — present
    /// whenever a TLS handshake completed at all (valid or not).
    pub fingerprint: Option<String>,
}

pub fn fingerprint_hex(der: &[u8]) -> String {
    hex::encode(Sha256::digest(der))
}

/// Accepts any certificate and records the end-entity DER fingerprint.
/// Only ever used for the *probe* (no credentials ride on it) and for
/// pinned transport where the fingerprint is compared before acceptance.
#[derive(Debug)]
pub struct CapturingVerifier {
    pub seen: Arc<Mutex<Option<String>>>,
    /// When set, the handshake is refused unless the fingerprint matches.
    pub require: Option<String>,
    provider: Arc<rustls::crypto::CryptoProvider>,
}

impl CapturingVerifier {
    pub fn new(require: Option<String>) -> Self {
        Self {
            seen: Arc::new(Mutex::new(None)),
            require,
            provider: Arc::new(rustls::crypto::ring::default_provider()),
        }
    }
}

impl ServerCertVerifier for CapturingVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        let fp = fingerprint_hex(end_entity.as_ref());
        // A poisoned lock would mean another thread panicked mid-handshake;
        // the fingerprint is still the one thing worth keeping, so take it.
        match self.seen.lock() {
            Ok(mut seen) => *seen = Some(fp.clone()),
            Err(poisoned) => *poisoned.into_inner() = Some(fp.clone()),
        }
        if let Some(required) = &self.require {
            if &fp != required {
                return Err(rustls::Error::General("pinned fingerprint mismatch".into()));
            }
        }
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

/// A hub that never answers must not leave the Add Hub button spinning; the
/// probe is a look at a handshake, not a download.
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);

/// A client whose only certificate judgement is the one this verifier makes.
pub fn client_with_verifier(verifier: Arc<CapturingVerifier>) -> Result<reqwest::Client, String> {
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|_| "Could not set up a TLS client.".to_string())?
    .dangerous()
    .with_custom_certificate_verifier(verifier)
    .with_no_client_auth();
    reqwest::Client::builder()
        .use_preconfigured_tls(config)
        .timeout(PROBE_TIMEOUT)
        .build()
        .map_err(|_| "Could not set up a TLS client.".to_string())
}

/// The probe itself: is this hub's certificate one the system already trusts,
/// and — trusted or not — which certificate is it? The fingerprint is captured
/// either way, so a user who decides to pin does not pay a second round-trip.
pub async fn probe_url(url: &str) -> Result<ProbeReport, String> {
    if url.starts_with("http://") {
        // Trust for plain http is the user's checkbox, not a handshake: there
        // is nothing here to look at and nothing to ask the network.
        return Ok(ProbeReport {
            https_valid: false,
            fingerprint: None,
        });
    }
    // First: does the system trust store accept it?
    let valid = match reqwest::Client::builder().timeout(PROBE_TIMEOUT).build() {
        Ok(client) => client.get(url).send().await.is_ok(),
        Err(_) => false,
    };
    let verifier = Arc::new(CapturingVerifier::new(None));
    let client = client_with_verifier(verifier.clone())?;
    // Any response — even a 404 — means the handshake completed and the
    // fingerprint was captured. A transport error with no captured cert means
    // the host is unreachable; surface reqwest's words.
    let attempt = client.get(url).send().await;
    let fingerprint = verifier
        .seen
        .lock()
        .map(|seen| seen.clone())
        .unwrap_or_else(|poisoned| poisoned.into_inner().clone());
    if fingerprint.is_none() {
        if let Err(e) = attempt {
            return Err(e.to_string());
        }
    }
    Ok(ProbeReport {
        https_valid: valid,
        fingerprint,
    })
}

#[tauri::command]
pub async fn probe_hub(url: String) -> Result<ProbeReport, String> {
    probe_url(&url).await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The one thing a test can say about the fingerprint without a TLS server:
    /// it is the SHA-256 of the bytes it was handed, in lowercase hex.
    fn sha256_hex_of(bytes: &[u8]) -> String {
        use sha2::{Digest, Sha256};
        let digest = Sha256::digest(bytes);
        digest.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn http_urls_probe_to_no_tls_without_any_request() {
        // Port 1 answers nothing: if the probe reached the network at all this
        // would come back as a transport error rather than a report.
        let report = tauri::async_runtime::block_on(probe_url("http://127.0.0.1:1")).unwrap();
        assert!(!report.https_valid);
        assert!(report.fingerprint.is_none());
    }

    #[test]
    fn fingerprint_is_lowercase_hex_of_der_sha256() {
        assert_eq!(fingerprint_hex(b"abc"), sha256_hex_of(b"abc"));
        assert_eq!(fingerprint_hex(b"abc").len(), 64);
    }
}
