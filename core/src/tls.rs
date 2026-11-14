//! Only Proton's own keys are accepted.
//!
//! A certificate authority that issues a certificate for proton.me, by mistake or under pressure, still does not
//! get to read the session. The pins are the ones Proton's Bridge trusts (`TrustedAPIPins`): when Proton retires
//! all three keys, this needs a release, and until then the client reports that it cannot reach Proton.

use base64::{Engine, engine::general_purpose::STANDARD};
use rustls::{
    ClientConfig, DigitallySignedStruct, Error, RootCertStore, SignatureScheme,
    client::{
        WebPkiServerVerifier,
        danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    },
    crypto::ring,
    pki_types::{CertificateDer, ServerName, UnixTime},
};
use sha2::{Digest, Sha256};
use std::sync::Arc;

/// proton.me: current, hot backup, cold backup.
pub const PROTON_PINS: &[&str] = &[
    "CT56BhOTmj5ZIPgb/xD5mH8rY3BLo/MlhP7oPyJUEDo=",
    "35Dx28/uzN3LeltkCBQ8RHK0tlNSa2kCpCRGNp34Gxc=",
    "qYIukVc63DEITct8sFT7ebIq5qsWmuscaIKeJx+5J5A=",
];

/// Proton's onion site: its own key, and the key of the authority that issued it.
///
/// A renewed certificate from the same authority still passes. The onion address itself already proves whose server
/// it is.
pub const ONION_PINS: &[&str] =
    &["fRKWzOayDIxBK43Cha3aUjFC/q0J7A2y6WUrycM/hpY=", "w9I8WrpHr1YAS0DugvLGsrI9Vm1yEkZAyllKxemXXf4="];

/// Both servers speak it, and leaving nothing older out there leaves nothing to be downgraded to.
const ONLY_TLS13: &[&rustls::SupportedProtocolVersion] = &[&rustls::version::TLS13];

const UNTRUSTED_KEY: &str = "the server's key is not one of Proton's";

/// Whether a failed connection was refused by these checks, and not merely lost. A wrong key means
/// someone may be in the way, and calling that "no connection" would hide it.
pub fn is_refused_certificate(error: &(dyn std::error::Error + 'static)) -> bool {
    let mut current = Some(error);
    while let Some(error) = current {
        if let Some(refused) = error.downcast_ref::<Error>() {
            return matches!(refused, Error::InvalidCertificate(_))
                || matches!(refused, Error::General(text) if text == UNTRUSTED_KEY);
        }
        let inner = error.downcast_ref::<std::io::Error>().and_then(std::io::Error::get_ref);
        current = inner.map_or_else(|| error.source(), |inner| Some(inner as &(dyn std::error::Error + 'static)));
    }
    false
}

fn pin_of(certificate: &CertificateDer<'_>) -> Option<String> {
    let parsed = webpki::EndEntityCert::try_from(certificate).ok()?;
    Some(STANDARD.encode(Sha256::digest(parsed.subject_public_key_info().as_ref())))
}

/// The usual checks of the chain and the name, and then the key.
#[derive(Debug)]
struct Pinned {
    usual: Arc<WebPkiServerVerifier>,
    pins: &'static [&'static str],
    /// Whether an issuer's key may match too, and not only the server's own.
    issuers_too: bool,
}

impl ServerCertVerifier for Pinned {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, Error> {
        let verified = self.usual.verify_server_cert(end_entity, intermediates, server_name, ocsp_response, now)?;
        let issuers = if self.issuers_too { intermediates } else { &[] };
        let pinned =
            std::iter::once(end_entity).chain(issuers).filter_map(pin_of).any(|pin| self.pins.contains(&pin.as_str()));
        if pinned { Ok(verified) } else { Err(Error::General(UNTRUSTED_KEY.to_string())) }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        certificate: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        self.usual.verify_tls12_signature(message, certificate, signature)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        certificate: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        self.usual.verify_tls13_signature(message, certificate, signature)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.usual.supported_verify_schemes()
    }
}

/// What Quad9's certificate may chain to. Quad9 publishes no pins and its certificates are renewed
/// yearly, so the trust is narrowed to one company's roots, the current one and its successors,
/// instead of every authority there is. The chain itself is checked as usual.
const QUAD9_ROOTS: &[&str] = &[
    "DigiCert Global Root G2",
    "DigiCert Global Root G3",
    "DigiCert TLS ECC P384 Root G5",
    "DigiCert TLS RSA4096 Root G5",
];

fn quad9_roots() -> RootCertStore {
    let names =
        |subject: &[u8]| QUAD9_ROOTS.iter().any(|name| subject.windows(name.len()).any(|part| part == name.as_bytes()));
    let mut roots = RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().filter(|root| names(root.subject.as_ref())).cloned());
    roots
}

/// Quad9 answers DNS over HTTP/2 only.
pub fn quad9_config() -> Result<ClientConfig, Error> {
    let provider = Arc::new(ring::default_provider());
    let mut config = ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(ONLY_TLS13)?
        .with_root_certificates(quad9_roots())
        .with_no_client_auth();
    config.alpn_protocols = vec![b"h2".to_vec()];
    Ok(config)
}

fn pinned_verifier(pins: &'static [&'static str], issuers_too: bool, roots: RootCertStore) -> Result<Pinned, Error> {
    let provider = Arc::new(ring::default_provider());
    let usual = WebPkiServerVerifier::builder_with_provider(Arc::new(roots), provider)
        .build()
        .map_err(|e| Error::General(e.to_string()))?;
    Ok(Pinned { usual, pins, issuers_too })
}

pub fn pinned_config(pins: &'static [&'static str]) -> Result<ClientConfig, Error> {
    pinned_config_with(pins, false)
}

/// For Proton's onion site, where the issuer's key counts as well.
pub fn onion_config() -> Result<ClientConfig, Error> {
    pinned_config_with(ONION_PINS, true)
}

fn pinned_config_with(pins: &'static [&'static str], issuers_too: bool) -> Result<ClientConfig, Error> {
    let mut roots = RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let verifier = pinned_verifier(pins, issuers_too, roots)?;
    let mut config = ClientConfig::builder_with_provider(Arc::new(ring::default_provider()))
        .with_protocol_versions(ONLY_TLS13)?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(verifier))
        .with_no_client_auth();
    // One connection for everything: a handshake for each parallel request costs more than its answer.
    config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    Ok(config)
}

#[cfg(test)]
mod tests;
