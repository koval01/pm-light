use super::*;

#[test]
fn proton_is_asked_for_http2_first() {
    let config = pinned_config(PROTON_PINS).unwrap();
    assert_eq!(config.alpn_protocols.first().map(Vec::as_slice), Some(&b"h2"[..]));
}

/// What `mail-api.proton.me` presented when the pins were checked. Public, like any certificate.
const LEAF: &str = include_str!("../test_leaf.pem");

fn leaf() -> CertificateDer<'static> {
    let body: String = LEAF.lines().filter(|line| !line.starts_with("-----")).collect();
    CertificateDer::from(STANDARD.decode(body).unwrap())
}

#[test]
fn hashes_the_key_as_proton_publishes_it() {
    assert_eq!(pin_of(&leaf()).as_deref(), Some(PROTON_PINS[0]));
}

#[test]
fn junk_has_no_pin() {
    assert_eq!(pin_of(&CertificateDer::from(vec![1, 2, 3])), None);
}

/// A certificate authority the client trusts, but that is not Proton: one that was hacked, or
/// made to obey. It issues a perfectly valid certificate for Proton's own name.
fn rogue_authority(host: &str) -> (RootCertStore, CertificateDer<'static>) {
    let mut authority = rcgen::CertificateParams::new(Vec::new()).unwrap();
    authority.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    let authority_key = rcgen::KeyPair::generate().unwrap();
    let authority = authority.self_signed(&authority_key).unwrap();
    let params = rcgen::CertificateParams::new(vec![host.to_string()]).unwrap();
    let leaf = params.signed_by(&rcgen::KeyPair::generate().unwrap(), &authority, &authority_key).unwrap();
    let mut roots = RootCertStore::empty();
    roots.add(authority.der().clone()).unwrap();
    (roots, leaf.der().clone())
}

fn verify(verifier: &Pinned, leaf: &CertificateDer<'static>, host: &'static str) -> Result<ServerCertVerified, Error> {
    let name = ServerName::try_from(host).unwrap();
    verifier.verify_server_cert(leaf, &[], &name, &[], UnixTime::now())
}

/// A root, an issuer under it, and a server's certificate from the issuer, as Proton's onion site has.
fn chain(host: &str) -> (RootCertStore, CertificateDer<'static>, CertificateDer<'static>) {
    let mut root = rcgen::CertificateParams::new(Vec::new()).unwrap();
    root.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    let root_key = rcgen::KeyPair::generate().unwrap();
    let root = root.self_signed(&root_key).unwrap();
    let mut issuer = rcgen::CertificateParams::new(Vec::new()).unwrap();
    issuer.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    let issuer_key = rcgen::KeyPair::generate().unwrap();
    let issuer = issuer.signed_by(&issuer_key, &root, &root_key).unwrap();
    let leaf = rcgen::CertificateParams::new(vec![host.to_string()]).unwrap();
    let leaf = leaf.signed_by(&rcgen::KeyPair::generate().unwrap(), &issuer, &issuer_key).unwrap();
    let mut roots = RootCertStore::empty();
    roots.add(root.der().clone()).unwrap();
    (roots, leaf.der().clone(), issuer.der().clone())
}

#[test]
fn the_onion_site_may_match_by_its_issuer_and_nothing_else_may() {
    let host = "mail-api.example.onion";
    let (roots, leaf, issuer) = chain(host);
    let pin: &'static str = Box::leak(pin_of(&issuer).unwrap().into_boxed_str());
    let pins: &'static [&'static str] = Box::leak(vec![pin].into_boxed_slice());
    let name = ServerName::try_from(host).unwrap();
    let onion = pinned_verifier(pins, true, roots.clone()).unwrap();
    assert!(onion.verify_server_cert(&leaf, std::slice::from_ref(&issuer), &name, &[], UnixTime::now()).is_ok());
    let proton = pinned_verifier(pins, false, roots).unwrap();
    assert!(proton.verify_server_cert(&leaf, &[issuer], &name, &[], UnixTime::now()).is_err());
}

#[test]
fn the_onion_pins_are_its_key_and_its_issuers() {
    assert_eq!(ONION_PINS.len(), 2);
    assert!(onion_config().is_ok());
}

#[test]
fn a_certificate_from_a_trusted_authority_is_still_refused_without_the_key() {
    let (roots, leaf) = rogue_authority("mail-api.proton.me");
    let verifier = pinned_verifier(PROTON_PINS, false, roots).unwrap();
    let refused = verify(&verifier, &leaf, "mail-api.proton.me").unwrap_err();
    assert!(matches!(refused, Error::General(text) if text == UNTRUSTED_KEY));
}

#[test]
fn that_refusal_comes_from_the_key_and_not_from_the_chain() {
    let (roots, leaf) = rogue_authority("mail-api.proton.me");
    let pin: &'static str = Box::leak(pin_of(&leaf).unwrap().into_boxed_str());
    let pins: &'static [&'static str] = Box::leak(vec![pin].into_boxed_slice());
    let verifier = pinned_verifier(pins, false, roots).unwrap();
    assert!(verify(&verifier, &leaf, "mail-api.proton.me").is_ok());
}

#[test]
fn the_right_key_on_the_wrong_name_is_refused_too() {
    let (roots, leaf) = rogue_authority("evil.example");
    let pin: &'static str = Box::leak(pin_of(&leaf).unwrap().into_boxed_str());
    let pins: &'static [&'static str] = Box::leak(vec![pin].into_boxed_slice());
    let verifier = pinned_verifier(pins, false, roots).unwrap();
    assert!(verify(&verifier, &leaf, "mail-api.proton.me").is_err());
}

/// A server that will only speak TLS 1.2, as one forcing a downgrade would.
#[test]
fn both_connections_refuse_anything_older_than_tls13() {
    let key = rcgen::KeyPair::generate().unwrap();
    let params = rcgen::CertificateParams::new(vec!["mail-api.proton.me".to_string()]).unwrap();
    let certificate = params.self_signed(&key).unwrap();
    let private = rustls::pki_types::PrivateKeyDer::try_from(key.serialize_der()).unwrap();
    let old = rustls::ServerConfig::builder_with_provider(Arc::new(ring::default_provider()))
        .with_protocol_versions(&[&rustls::version::TLS12])
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(vec![certificate.der().clone()], private)
        .unwrap();
    for config in [pinned_config(PROTON_PINS).unwrap(), quad9_config().unwrap()] {
        let name = ServerName::try_from("mail-api.proton.me").unwrap();
        let mut client = rustls::ClientConnection::new(Arc::new(config), name).unwrap();
        let mut server = rustls::ServerConnection::new(Arc::new(old.clone())).unwrap();
        let mut hello = Vec::new();
        client.write_tls(&mut hello).unwrap();
        server.read_tls(&mut hello.as_slice()).unwrap();
        assert!(matches!(server.process_new_packets(), Err(Error::PeerIncompatible(_))));
    }
}

#[test]
fn quad9_may_chain_only_to_the_named_roots() {
    assert_eq!(quad9_roots().len(), QUAD9_ROOTS.len());
}

#[test]
fn a_refused_certificate_is_told_from_a_lost_connection() {
    let refused = Error::General(UNTRUSTED_KEY.to_string());
    assert!(is_refused_certificate(&refused));
    let wrapped = std::io::Error::other(Error::InvalidCertificate(rustls::CertificateError::Expired));
    assert!(is_refused_certificate(&wrapped));
    let lost = std::io::Error::from(std::io::ErrorKind::ConnectionReset);
    assert!(!is_refused_certificate(&lost));
    assert!(!is_refused_certificate(&Error::General("something else".to_string())));
}

fn client(pins: &'static [&'static str]) -> reqwest::Client {
    reqwest::Client::builder().use_preconfigured_tls(pinned_config(pins).unwrap()).build().unwrap()
}

/// Reaches Proton, so it is not part of the sandboxed run. See "Attacks" in docs/DEVELOPMENT.md.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "reaches Proton"]
async fn live_the_pins_accept_proton_and_refuse_anything_else() {
    let url = "https://mail-api.proton.me/tests/ping";
    assert!(client(PROTON_PINS).get(url).send().await.is_ok());
    assert!(client(&["AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="]).get(url).send().await.is_err());
}
