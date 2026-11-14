use super::*;
use hickory_proto::rr::rdata::{A, CNAME, MX, TXT};

fn answer(data: RData, ttl: u32) -> Record {
    Record::from_rdata(Name::from_ascii("example.com.").unwrap(), ttl, data)
}

fn reply(code: ResponseCode, records: Vec<Record>) -> Vec<u8> {
    let mut message = Message::response(0, OpCode::Query);
    message.metadata.response_code = code;
    message.add_answers(records);
    message.to_vec().unwrap()
}

#[test]
fn the_journal_names_only_protons_hosts() {
    assert!(is_protons("mail-api.proton.me") && is_protons("proton.me.") && is_protons("x.protonmail.ch"));
    assert!(!is_protons("proton.me.evil.example") && !is_protons("notproton.me") && !is_protons("gmail.com"));
    let mx = answer(RData::MX(MX::new(10, Name::from_ascii("mx.corp.example.").unwrap())), 60);
    let address = answer(RData::A(A::new(185, 70, 42, 36)), 300);
    let private = summary(&[mx, address.clone(), address.clone()], false);
    assert_eq!(private, "1 MX, 2 A");
    assert!(!private.contains("corp"));
    assert_eq!(summary(&[address], true), "A 185.70.42.36 (ttl 300 s)");
    assert_eq!(summary(&[], true), "nothing");
}

#[test]
fn mail_needs_a_host_or_an_address() {
    let host = |name: &str| vec![(10, name.to_string())];
    assert!(takes_mail(&host("mail.example.org."), false));
    assert!(takes_mail(&[], true));
    assert!(!takes_mail(&[], false));
    assert!(!takes_mail(&host("."), true));
}

#[test]
fn a_query_is_a_real_dns_message() {
    let bytes = encode_query("mail-api.proton.me", RecordType::A).unwrap();
    let message = Message::from_vec(&bytes).unwrap();
    assert!(message.metadata.recursion_desired);
    assert_eq!(message.queries[0].name().to_ascii().trim_end_matches('.'), "mail-api.proton.me");
}

#[test]
fn a_broken_name_is_refused_before_asking() {
    assert!(encode_query(&"a".repeat(300), RecordType::A).is_err());
}

#[test]
fn answers_are_read_by_kind() {
    let records = vec![
        answer(RData::A(A::new(1, 2, 3, 4)), 60),
        answer(RData::TXT(TXT::new(vec!["v=spf1 ".into(), "~all".into()])), 60),
        answer(RData::MX(MX::new(10, Name::from_ascii("mail.protonmail.ch.").unwrap())), 60),
        answer(RData::CNAME(CNAME(Name::from_ascii("a.example.").unwrap())), 60),
    ];
    let read = decode_answer(&reply(ResponseCode::NoError, records)).unwrap();
    assert_eq!(addresses(&read), [IpAddr::V4(Ipv4Addr::new(1, 2, 3, 4))]);
    assert_eq!(texts(&read), ["v=spf1 ~all"]);
    assert_eq!(mail_hosts(&read), [(10, "mail.protonmail.ch.".to_string())]);
    assert_eq!(aliases(&read), ["a.example."]);
}

#[test]
fn a_missing_name_is_an_empty_answer_and_a_failure_is_not() {
    assert!(decode_answer(&reply(ResponseCode::NXDomain, vec![])).unwrap().is_empty());
    assert!(matches!(decode_answer(&reply(ResponseCode::ServFail, vec![])), Err(Error::Network(_))));
    assert!(matches!(decode_answer(&reply(ResponseCode::Refused, vec![])), Err(Error::Network(_))));
}

#[test]
fn junk_cannot_panic_the_reader() {
    for junk in [&[][..], &[0xff; 12], &[0; 2048], b"<html>"] {
        let _ = decode_answer(junk);
    }
}

/// Someone on the network answers for Quad9 with a certificate no DigiCert root signed.
#[tokio::test(flavor = "multi_thread")]
async fn a_false_quad9_gets_no_query() {
    let (address, reached) = crate::test_server::false_server("dns.quad9.net");
    let quad9 = Quad9::at(&[address]).unwrap();
    assert!(matches!(quad9.txt("example.com").await, Err(Error::Untrusted)));
    assert!(!reached.load(std::sync::atomic::Ordering::Relaxed), "a query reached the false server");
}

#[test]
fn nothing_is_cached_for_too_short_or_too_long() {
    let record = |ttl| answer(RData::A(A::new(1, 1, 1, 1)), ttl);
    assert_eq!(seconds_to_keep(&[record(1)]), MIN_CACHE_SECONDS);
    assert_eq!(seconds_to_keep(&[record(86_400)]), MAX_CACHE_SECONDS);
    assert_eq!(seconds_to_keep(&[record(120), record(90)]), 90);
}

/// Reaches Quad9 and Proton, so it is not part of the sandboxed run.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "reaches Quad9"]
async fn live_quad9_resolves_through_doh() {
    let quad9 = Quad9::new().unwrap();
    let found = Quad9::addresses_of(quad9.http.client(), quad9.cache.clone(), "mail-api.proton.me".into());
    assert!(!found.await.unwrap().is_empty());
    assert!(quad9.txt("proton.me").await.unwrap().iter().any(|text| text.starts_with("v=spf1")));
}

/// A real, valid site under another authority stands in for a false Quad9 with a real certificate.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "reaches Proton"]
async fn live_the_quad9_connection_refuses_other_authorities() {
    let http =
        reqwest::Client::builder().use_preconfigured_tls(tls::quad9_config().unwrap()).no_proxy().build().unwrap();
    let refused = http.get("https://letsencrypt.org/").send().await.unwrap_err();
    assert!(matches!(Error::from(refused), Error::Untrusted));
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "reaches Quad9"]
async fn live_quad9_is_refused_when_its_chain_is_not_trusted() {
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(
        webpki_roots::TLS_SERVER_ROOTS
            .iter()
            .filter(|r| !String::from_utf8_lossy(r.subject.as_ref()).contains("DigiCert"))
            .cloned(),
    );
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(roots)
        .with_no_client_auth();
    let http = reqwest::Client::builder()
        .use_preconfigured_tls(config)
        .resolve_to_addrs(HOST, &[SocketAddr::new(ADDRESSES[0], PORT)])
        .build()
        .unwrap();
    let refused = http.post(ENDPOINT).send().await.unwrap_err();
    assert!(matches!(Error::from(refused), Error::Untrusted));
}
