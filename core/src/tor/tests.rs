use super::socks::{Entry, Purpose, Target, handshake, is_allowed, may_go, same};
use super::{ONION_API, ONION_HOST, Route, patiently, percent_of};
use std::net::{IpAddr, Ipv4Addr};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

fn name(host: &str) -> Target {
    Target::Name(host.to_string())
}

const QUAD9: Target = Target::Address(IpAddr::V4(Ipv4Addr::new(9, 9, 9, 9)));

#[test]
fn each_purpose_may_go_only_where_it_belongs() {
    for host in ["mail-api.proton.me", "proton.me", "verify.proton.me.", "account.protonmail.com", ONION_HOST] {
        assert!(may_go(Purpose::Proton, &name(host), 443), "{host}");
        assert!(may_go(Purpose::Page, &name(host), 443), "{host}");
        assert!(!may_go(Purpose::Quad9, &name(host), 443), "{host}");
    }
    assert!(may_go(Purpose::Proton, &name(&format!("mail-api.{ONION_HOST}")), 443));
    for host in ["evil.example", "proton.me.evil.example", "notproton.me", "protonmailrmez3.onion", "dns.quad9.net"] {
        assert!(!may_go(Purpose::Proton, &name(host), 443), "{host}");
        assert!(!may_go(Purpose::Page, &name(host), 443), "{host}");
    }
    assert!(!may_go(Purpose::Proton, &name("mail-api.proton.me"), 80));
    assert!(may_go(Purpose::Quad9, &QUAD9, 443));
    assert!(!may_go(Purpose::Quad9, &QUAD9, 53));
    assert!(!may_go(Purpose::Proton, &QUAD9, 443));
    assert!(!may_go(Purpose::Quad9, &Target::Address(IpAddr::V4(Ipv4Addr::new(185, 70, 42, 41))), 443));
    assert!(is_allowed("MAIL-API.PROTON.ME"));
}

#[test]
fn the_onion_api_is_where_the_web_client_puts_it() {
    assert_eq!(ONION_API, format!("https://mail-api.{ONION_HOST}"));
    assert_eq!(Route::Onion.api(), ONION_API);
    assert_eq!(Route::Exit.api(), crate::client::PROTON_API);
    assert_eq!(Route::default(), Route::Onion);
}

#[test]
fn passwords_are_compared_whole() {
    assert!(same(b"secret", b"secret"));
    assert!(!same(b"secreT", b"secret"));
    assert!(!same(b"secret", b"secrets"));
    assert!(!same(b"", b"secret"));
}

#[test]
fn progress_stays_between_nothing_and_all() {
    assert_eq!(percent_of(0.0), 0);
    assert_eq!(percent_of(0.456), 46);
    assert_eq!(percent_of(1.7), 100);
    assert_eq!(percent_of(f32::NAN), 0);
}

/// Sends `bytes` as a client would and gives what the door made of them.
async fn asked(bytes: Vec<u8>, entry: Entry) -> Option<(Purpose, Target, u16)> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let address = listener.local_addr().unwrap();
    let client = tokio::spawn(async move {
        let mut stream = TcpStream::connect(address).await.unwrap();
        stream.write_all(&bytes).await.unwrap();
        // Nothing more is coming: a request cut short must end, not wait.
        stream.shutdown().await.unwrap();
        let mut answer = Vec::new();
        let _ = stream.read_to_end(&mut answer).await;
    });
    let (mut stream, _) = listener.accept().await.unwrap();
    let result = handshake(&mut stream, &entry).await.ok().flatten();
    drop(stream);
    client.await.unwrap();
    result
}

fn request(host: &str, port: u16) -> Vec<u8> {
    let mut bytes = vec![5, 1, 0, 3, u8::try_from(host.len()).unwrap()];
    bytes.extend_from_slice(host.as_bytes());
    bytes.extend_from_slice(&port.to_be_bytes());
    bytes
}

fn with_password(name: &str, password: &str, then: &[u8]) -> Vec<u8> {
    let mut bytes = vec![5, 1, 2, 1, u8::try_from(name.len()).unwrap()];
    bytes.extend_from_slice(name.as_bytes());
    bytes.push(u8::try_from(password.len()).unwrap());
    bytes.extend_from_slice(password.as_bytes());
    bytes.extend_from_slice(then);
    bytes
}

fn doors() -> Entry {
    Entry::Passwords(vec![
        ("p".to_string(), "proton-word".to_string(), Purpose::Proton),
        ("q".to_string(), "quad9-word".to_string(), Purpose::Quad9),
    ])
}

#[tokio::test]
async fn the_name_given_chooses_the_circuits() {
    let proton = with_password("p", "proton-word", &request("mail-api.proton.me", 443));
    assert_eq!(asked(proton, doors()).await, Some((Purpose::Proton, name("mail-api.proton.me"), 443)));
    let quad9 = with_password("q", "quad9-word", &request("mail-api.proton.me", 443));
    assert_eq!(asked(quad9, doors()).await, Some((Purpose::Quad9, name("mail-api.proton.me"), 443)));
}

#[tokio::test]
async fn a_wrong_password_or_none_is_turned_away() {
    for (user, word) in [("p", "guess"), ("p", "quad9-word"), ("q", "proton-word"), ("x", "proton-word")] {
        let wrong = with_password(user, word, &request("mail-api.proton.me", 443));
        assert_eq!(asked(wrong, doors()).await, None, "{user} {word}");
    }
    let mut without = vec![5, 1, 0];
    without.extend(request("mail-api.proton.me", 443));
    assert_eq!(asked(without, doors()).await, None);
}

#[tokio::test]
async fn a_page_needs_no_password_but_only_connect_is_done() {
    let mut connect = vec![5, 1, 0];
    connect.extend(request("verify.proton.me", 443));
    assert_eq!(asked(connect, Entry::Open(Purpose::Page)).await, Some((Purpose::Page, name("verify.proton.me"), 443)));
    let mut bind = vec![5, 1, 0, 5, 2, 0, 3, 3];
    bind.extend_from_slice(b"x.y");
    bind.extend_from_slice(&443u16.to_be_bytes());
    assert_eq!(asked(bind, Entry::Open(Purpose::Page)).await, None);
}

#[tokio::test]
async fn nonsense_is_turned_away() {
    for junk in [vec![4, 1, 0], vec![5, 0], vec![5, 1, 0, 5, 1, 0, 9], vec![], vec![5, 1, 0, 5, 1, 0, 3, 0, 1, 187]] {
        assert_eq!(asked(junk.clone(), Entry::Open(Purpose::Page)).await, None, "{junk:?}");
    }
}

/// Real Tor, in a folder of its own: Proton's onion site answers through it, the circuit's countries are known, and
/// the door refuses a host that is not Proton's even with the right password. Reaches the Tor network and Proton, so
/// it is not part of the sandboxed run.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "reaches Tor"]
async fn live_tor_reaches_proton_and_nothing_else() {
    use crate::client::{Client, Way};
    let folder = std::env::temp_dir().join(format!("pml-tor-test-{}", std::process::id()));
    let tor = super::Tor::start(&folder).await.unwrap();
    for route in [Route::Onion, Route::Exit] {
        let client = Client::undecided("https://unused.example");
        client.decide(&Way::Tor { tor: tor.clone(), route }).unwrap();
        assert!(client.through_tor());
        let pong: serde_json::Value = client.get("/tests/ping").await.unwrap();
        assert_eq!(pong["Code"], 1000, "{route:?}");
        let circuit = tor.status().circuit;
        assert!(circuit.len() >= 3, "{route:?}: {circuit:?}");
    }
    assert_eq!(tor.status().state, super::TorState::Ready);
    // The right password, asking for somewhere else: the door says no.
    let proxy = reqwest::Proxy::all(tor.proxy_for_proton()).unwrap();
    let outside = reqwest::Client::builder().proxy(proxy).build().unwrap();
    assert!(outside.get("https://example.com/").send().await.is_err());
    let _ = std::fs::remove_dir_all(folder);
}

/// The app's own start: Tor's folder already holds a directory from an earlier start, and the first request goes out
/// at once, before Tor can carry it. It must wait for Tor, not fail: a failure here put the session on the waiting
/// screen for a few seconds at every start.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "reaches Tor"]
async fn live_tor_a_request_at_once_after_a_start_with_a_cache_waits_for_tor() {
    use crate::client::{Client, Way};
    let folder = std::env::temp_dir().join(format!("pml-tor-cache-test-{}", std::process::id()));
    {
        let tor = super::Tor::start(&folder).await.unwrap();
        let client = Client::undecided("https://unused.example");
        client.decide(&Way::Tor { tor: tor.clone(), route: Route::Onion }).unwrap();
        let _: serde_json::Value = client.get("/tests/ping").await.unwrap();
    }
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    for attempt in 0..3 {
        let tor = super::Tor::start(&folder).await.unwrap();
        let client = Client::undecided("https://unused.example");
        client.decide(&Way::Tor { tor: tor.clone(), route: Route::Onion }).unwrap();
        let pong: crate::error::Result<serde_json::Value> = client.get("/tests/ping").await;
        assert!(pong.is_ok(), "attempt {attempt}: {:?}", pong.err().map(|error| error.kind()));
        drop(tor);
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
    let _ = std::fs::remove_dir_all(folder);
}

/// An answer from Tor, by its kind alone.
struct Refusal(arti_client::ErrorKind);

impl arti_client::HasKind for Refusal {
    fn kind(&self) -> arti_client::ErrorKind {
        self.0
    }
}

const MS: std::time::Duration = std::time::Duration::from_millis(1);

/// Tor not ready yet is waited out: the stream comes once it is.
#[tokio::test]
async fn a_stream_waits_while_tor_is_not_ready() {
    let mut tries = 0;
    let stream = patiently(MS * 2000, MS * 5, || {
        tries += 1;
        let answer = if tries < 4 { Err(Refusal(arti_client::ErrorKind::BootstrapRequired)) } else { Ok(tries) };
        async move { answer }
    })
    .await;
    assert_eq!(stream.ok(), Some(4));
}

/// Any other refusal is answered at once, without asking again.
#[tokio::test]
async fn another_refusal_is_not_asked_again() {
    for kind in [
        arti_client::ErrorKind::TorNetworkTimeout,
        arti_client::ErrorKind::OnionServiceNotFound,
        arti_client::ErrorKind::RemoteConnectionRefused,
        arti_client::ErrorKind::Other,
    ] {
        let mut tries = 0;
        let stream: crate::error::Result<()> = patiently(MS * 2000, MS * 5, || {
            tries += 1;
            async move { Err(Refusal(kind)) }
        })
        .await;
        assert!(stream.is_err(), "{kind:?}");
        assert_eq!(tries, 1, "{kind:?}");
    }
}

/// Waiting has an end: a Tor that never gets ready fails the stream within the limit.
#[tokio::test]
async fn waiting_for_tor_ends_at_the_limit() {
    let started = std::time::Instant::now();
    let stream: crate::error::Result<()> =
        patiently(MS * 200, MS * 20, || async { Err(Refusal(arti_client::ErrorKind::BootstrapRequired)) }).await;
    assert!(stream.is_err());
    assert!(started.elapsed() < MS * 400, "{:?}", started.elapsed());
}

/// An attempt that hangs is cut at the limit too.
#[tokio::test]
async fn a_hanging_attempt_ends_at_the_limit() {
    let started = std::time::Instant::now();
    let stream: crate::error::Result<()> =
        patiently(MS * 100, MS * 10, std::future::pending::<std::result::Result<(), Refusal>>).await;
    assert!(stream.is_err());
    assert!(started.elapsed() < MS * 300, "{:?}", started.elapsed());
}

/// New circuits leave nothing on the old ones: the client's kept-alive connection is cut, so the next request opens
/// a stream over a new circuit, and the window has a circuit to show again. Without the cut the next request went
/// over the old connection, no stream was opened, and the window showed no circuit at all.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "reaches Tor"]
async fn live_tor_new_circuits_cut_the_connections_kept_alive() {
    use crate::client::{Client, Way};
    let folder = std::env::temp_dir().join(format!("pml-tor-renew-test-{}", std::process::id()));
    let tor = super::Tor::start(&folder).await.unwrap();
    let client = Client::undecided("https://unused.example");
    client.decide(&Way::Tor { tor: tor.clone(), route: Route::Onion }).unwrap();
    let _: serde_json::Value = client.get("/tests/ping").await.unwrap();
    assert!(!tor.status().hops.is_empty());
    for round in 0..2 {
        tor.new_circuits();
        assert!(tor.status().hops.is_empty());
        let pong: crate::error::Result<serde_json::Value> = client.get("/tests/ping").await;
        assert!(pong.is_ok(), "round {round}: {:?}", pong.err().map(|error| error.kind()));
        let hops = tor.status().hops;
        assert!(hops.len() >= 3, "round {round}: {hops:?}");
        assert!(hops.iter().all(|hop| !hop.address.is_empty()), "round {round}: {hops:?}");
    }
    let _ = std::fs::remove_dir_all(folder);
}

/// A relay the directory gives no country for is placed by its address, in the database Tor itself carries: a
/// middle relay at 65.108.72.156 was shown as `??` while the database knew it.
#[test]
fn a_relay_without_a_country_in_the_directory_is_placed_by_its_address() {
    let known = |address: &str| super::country_by_address(address.parse().ok());
    assert_eq!(known("65.108.72.156"), Some("FI".to_string()));
    assert_eq!(known("178.162.221.111"), Some("DE".to_string()));
    assert_eq!(known("127.0.0.1"), None);
    assert_eq!(super::country_by_address(None), None);
}

/// What a SOCKS server is asked for by the app's own HTTP client, read off the wire: the kind of address, and the
/// address. Answers the greeting and the password, then closes.
async fn what_the_client_asks(proxy_of: impl FnOnce(std::net::SocketAddr) -> String) -> Option<(u8, Vec<u8>)> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let door = listener.local_addr().unwrap();
    let proxy = proxy_of(door);
    let quad9 = std::sync::Arc::new(crate::dns::Quad9::new().unwrap());
    let tls = crate::tls::onion_config().unwrap();
    let client = crate::client::http_client_with("https://mail-api.proton.me", tls, Some(&proxy), &quad9).unwrap();
    let asking = tokio::spawn(async move { client.get("https://mail-api.proton.me/tests/ping").send().await });
    let read = async {
        let (mut stream, _) = listener.accept().await.ok()?;
        let mut greeting = [0u8; 2];
        stream.read_exact(&mut greeting).await.ok()?;
        let mut methods = vec![0u8; usize::from(greeting[1])];
        stream.read_exact(&mut methods).await.ok()?;
        stream.write_all(&[5, 2]).await.ok()?;
        let mut head = [0u8; 2];
        stream.read_exact(&mut head).await.ok()?;
        let mut name = vec![0u8; usize::from(head[1])];
        stream.read_exact(&mut name).await.ok()?;
        let mut length = [0u8; 1];
        stream.read_exact(&mut length).await.ok()?;
        let mut password = vec![0u8; usize::from(length[0])];
        stream.read_exact(&mut password).await.ok()?;
        stream.write_all(&[1, 0]).await.ok()?;
        let mut request = [0u8; 4];
        stream.read_exact(&mut request).await.ok()?;
        let address = match request[3] {
            3 => {
                let mut length = [0u8; 1];
                stream.read_exact(&mut length).await.ok()?;
                let mut name = vec![0u8; usize::from(length[0])];
                stream.read_exact(&mut name).await.ok()?;
                name
            }
            1 => {
                let mut four = vec![0u8; 4];
                stream.read_exact(&mut four).await.ok()?;
                four
            }
            _ => Vec::new(),
        };
        Some((request[3], address))
    };
    let found = tokio::time::timeout(std::time::Duration::from_secs(10), read).await.ok().flatten();
    asking.abort();
    found
}

/// The app's client for Proton hands the host's name to Tor and never looks it up on this computer: with plain
/// `socks5` it would ask a resolver first, and every name it reaches would leave outside Tor.
#[tokio::test]
async fn protons_names_are_looked_up_at_the_far_end_of_the_circuit() {
    let secret = ("p".to_string(), "word".to_string());
    let asked = what_the_client_asks(|door| super::proxy_url(super::Names::InTor, &secret, door)).await;
    assert_eq!(asked, Some((3, b"mail-api.proton.me".to_vec())));
    assert!(super::proxy_url(super::Names::InTor, &secret, "127.0.0.1:1".parse().unwrap()).starts_with("socks5h://"));
}

/// The test above tells the two apart: a client that resolves names itself never sends one to the door.
#[tokio::test]
async fn a_client_resolving_names_itself_would_be_caught() {
    let secret = ("p".to_string(), "word".to_string());
    let asked = what_the_client_asks(|door| super::proxy_url(super::Names::None, &secret, door)).await;
    assert_ne!(asked.map(|(kind, _)| kind), Some(3));
}

/// The door is on this computer only: nothing on the network can use it as a way into Tor.
#[tokio::test]
async fn the_door_listens_on_this_computer_only() {
    let (address, _listener) = super::socks::listen().await.unwrap();
    assert!(address.ip().is_loopback() && address.port() != 0, "{address}");
    let (address, _listener) = super::socks::listen_now().unwrap();
    assert!(address.ip().is_loopback() && address.port() != 0, "{address}");
}

/// The door's passwords cannot be guessed by another program on this computer: 128 random bits each, new for every
/// start.
#[test]
fn the_doors_passwords_are_long_and_never_repeat() {
    let words: std::collections::BTreeSet<String> = (0..1000).map(|_| super::random_word()).collect();
    assert_eq!(words.len(), 1000);
    assert!(words.iter().all(|word| word.len() == 32 && word.bytes().all(|byte| byte.is_ascii_hexdigit())));
}

/// Sends `bytes` to the door as a whole connection, with no Tor behind it, and gives the door's answer.
async fn answered(bytes: Vec<u8>, entry: Entry) -> Vec<u8> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let address = listener.local_addr().unwrap();
    let client = tokio::spawn(async move {
        let mut stream = TcpStream::connect(address).await.unwrap();
        stream.write_all(&bytes).await.unwrap();
        stream.shutdown().await.unwrap();
        let mut answer = Vec::new();
        let _ = stream.read_to_end(&mut answer).await;
        answer
    });
    let (stream, _) = listener.accept().await.unwrap();
    let _ = super::socks::handle(stream, &std::sync::Weak::new(), &entry).await;
    client.await.unwrap()
}

/// A host that is not Proton's is refused on the wire, with SOCKS's "not allowed", before Tor is even asked.
#[tokio::test]
async fn a_host_that_is_not_protons_is_refused_on_the_wire() {
    let mut asking = vec![5, 1, 0];
    asking.extend(request("evil.example", 443));
    let answer = answered(asking, Entry::Open(Purpose::Page)).await;
    assert_eq!(answer.get(2..4), Some(&[5, 2][..]), "{answer:?}");
    let mut port = vec![5, 1, 0];
    port.extend(request("mail-api.proton.me", 80));
    let answer = answered(port, Entry::Open(Purpose::Page)).await;
    assert_eq!(answer.get(2..4), Some(&[5, 2][..]), "{answer:?}");
}

/// Hostile input at the door: random bytes, every length at its largest with the data cut short, names that are not
/// text, and a client that sends a byte and waits. The door answers or closes, in bounded time, and never lets
/// anything through without the password.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "hostile input: run by scripts/attacks.sh in a container"]
async fn hostile_input_at_the_door_is_turned_away_in_time() {
    use rand::{Rng, SeedableRng};
    let mut random = rand::rngs::StdRng::seed_from_u64(0x70_72_6f_74_6f_6e);
    let mut cases: Vec<Vec<u8>> = (0..3000)
        .map(|_| {
            let length = random.gen_range(0..600);
            let mut bytes: Vec<u8> = (0..length).map(|_| random.r#gen()).collect();
            if random.gen_bool(0.5) && !bytes.is_empty() {
                bytes[0] = 5;
            }
            bytes
        })
        .collect();
    cases.push(vec![5, 255]);
    cases.push(vec![5, 1, 2, 1, 255, b'p']);
    cases.push(vec![5, 1, 2, 1, 1, b'p', 255, b'x']);
    cases.push([vec![5, 1, 2, 1, 1, b'p', 4], b"word".to_vec(), vec![5, 1, 0, 3, 255, b'a']].concat());
    cases.push([vec![5, 1, 2, 1, 1, b'p', 4], b"word".to_vec(), vec![5, 1, 0, 3, 2, 0xff, 0xfe, 1, 187]].concat());
    cases.push([vec![5, 1, 2, 1, 1, b'p', 4], b"word".to_vec(), vec![5, 1, 0, 4], vec![0; 16], vec![1, 187]].concat());
    let started = std::time::Instant::now();
    let mut through = 0;
    for bytes in cases {
        let entry = Entry::Passwords(vec![("p".to_string(), "right-word".to_string(), Purpose::Proton)]);
        let answer = tokio::time::timeout(std::time::Duration::from_secs(2), answered(bytes.clone(), entry))
            .await
            .unwrap_or_else(|_| panic!("the door hung on {bytes:?}"));
        // A reply that says "succeeded" would mean a connection went through without the password.
        if answer.windows(2).any(|pair| pair == [5, 0]) && answer.len() >= 10 {
            through += 1;
        }
    }
    assert_eq!(through, 0);
    println!("3006 hostile openings turned away in {} ms", started.elapsed().as_millis());
    // A client that sends one byte and waits is dropped by the door's own limit.
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let address = listener.local_addr().unwrap();
    let slow = tokio::spawn(async move {
        let mut stream = TcpStream::connect(address).await.unwrap();
        stream.write_all(&[5]).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        drop(stream);
    });
    let (stream, _) = listener.accept().await.unwrap();
    let waited = std::time::Instant::now();
    let ended = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        super::socks::handle(stream, &std::sync::Weak::new(), &Entry::Open(Purpose::Page)),
    )
    .await;
    slow.abort();
    assert!(ended.is_ok_and(|result| result.is_err()), "a slow client kept the door");
    assert!(waited.elapsed() < std::time::Duration::from_secs(12));
}
