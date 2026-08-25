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
        // A probe answers about the host the user typed. Following a redirect
        // would let the answer describe a certificate from somewhere else
        // entirely — and an https→http hop would report "valid" for a hub
        // reached in the clear.
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "Could not set up a TLS client.".to_string())
}

/// Does the system trust store accept this host's certificate? One request,
/// judged only by whether it completed.
async fn system_trusts(url: &str) -> bool {
    match reqwest::Client::builder()
        .timeout(PROBE_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .build()
    {
        Ok(client) => client.get(url).send().await.is_ok(),
        Err(_) => false,
    }
}

/// The probe itself: is this hub's certificate one the system already trusts,
/// and — trusted or not — which certificate is it? The fingerprint is captured
/// either way, so a user who decides to pin does not pay a second round-trip.
pub async fn probe_url(url: &str) -> Result<ProbeReport, String> {
    // An allowlist, not a denial: only `https://` has a certificate to look at.
    // Everything else — plain http in any spelling, or a scheme we do not speak
    // — leaves without a request. Trust for plain http is the user's checkbox,
    // not a handshake, and a denial (`starts_with("http://")`) would have sent
    // `HTTP://hub.local` down the TLS path and reported a plaintext GET as valid.
    if !url.starts_with("https://") {
        return Ok(ProbeReport {
            https_valid: false,
            fingerprint: None,
        });
    }
    let verifier = Arc::new(CapturingVerifier::new(None));
    let client = client_with_verifier(verifier.clone())?;
    // The capturing handshake goes first, because it is what makes the trust
    // verdict below meaningful. It accepts any certificate, so any response —
    // even a 404 — means the host answered and the fingerprint was captured.
    let attempt = client.get(url).send().await;
    let fingerprint = verifier
        .seen
        .lock()
        .map(|seen| seen.clone())
        .unwrap_or_else(|poisoned| poisoned.into_inner().clone());
    let Some(fingerprint) = fingerprint else {
        // No handshake at all: the host is unreachable. Say so in reqwest's
        // words rather than reporting `https_valid: false`, which would read as
        // "this hub's certificate is untrusted" and offer to pin nothing.
        return match attempt {
            Err(e) => Err(e.to_string()),
            Ok(_) => Ok(ProbeReport {
                https_valid: false,
                fingerprint: None,
            }),
        };
    };
    // Now — and only now — a failed request against the system trust store is a
    // statement about *trust*. reqwest cannot tell us that: it reports a
    // rejected certificate, a refused connection and an unresolvable name all
    // as `is_connect()`, so the error kind separates nothing. The capturing
    // handshake above does: it proved the host is up and speaking TLS.
    //
    //   capturing failed          -> unreachable, returned above as an error
    //   capturing ok, system ok   -> https_valid: true
    //   capturing ok, system fails twice -> https_valid: false, a trust verdict
    //
    // The retry is for the third row: the host is known to be up, so one failed
    // request there is either a real verdict or a blip on that one request, and
    // a blip must not turn into a pin prompt the user has to judge.
    let https_valid = system_trusts(url).await || system_trusts(url).await;
    Ok(ProbeReport {
        https_valid,
        fingerprint: Some(fingerprint),
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
    fn only_https_is_probed_and_everything_else_leaves_without_a_request() {
        // Port 1 answers nothing: if the probe reached the network at all these
        // would come back as transport errors rather than reports. `HTTP://` is
        // the case a `starts_with("http://")` denial would have waved through
        // into the TLS path — a plaintext GET reported as a valid certificate.
        for url in [
            "http://127.0.0.1:1",
            "HTTP://127.0.0.1:1",
            "hTtP://127.0.0.1:1",
            "ftp://127.0.0.1:1",
        ] {
            let report = tauri::async_runtime::block_on(probe_url(url)).unwrap();
            assert!(!report.https_valid, "{url}");
            assert!(report.fingerprint.is_none(), "{url}");
        }
    }

    #[test]
    fn a_pinned_verifier_refuses_a_certificate_it_was_not_pinned_to() {
        // The compare happens on the fingerprint alone, before anything
        // certificate-shaped is needed — which is what lets Task 5's pinned
        // transport lean on it.
        let der = CertificateDer::from(b"not really a certificate".to_vec());
        let verifier = CapturingVerifier::new(Some("00".repeat(32)));
        let err = verifier
            .verify_server_cert(
                &der,
                &[],
                &ServerName::try_from("hub.example").unwrap(),
                &[],
                UnixTime::now(),
            )
            .unwrap_err();
        assert!(err.to_string().contains("pinned fingerprint mismatch"));
        // What it saw is recorded either way, so a refusal can still say which
        // certificate it refused.
        let seen = verifier.seen.lock().unwrap().clone();
        assert_eq!(
            seen.as_deref(),
            Some(fingerprint_hex(der.as_ref()).as_str())
        );

        // And the certificate it *was* pinned to is accepted.
        let matching = CapturingVerifier::new(Some(fingerprint_hex(der.as_ref())));
        assert!(matching
            .verify_server_cert(
                &der,
                &[],
                &ServerName::try_from("hub.example").unwrap(),
                &[],
                UnixTime::now(),
            )
            .is_ok());
    }

    #[test]
    fn fingerprint_is_lowercase_hex_of_der_sha256() {
        assert_eq!(fingerprint_hex(b"abc"), sha256_hex_of(b"abc"));
        assert_eq!(fingerprint_hex(b"abc").len(), 64);
    }
}
