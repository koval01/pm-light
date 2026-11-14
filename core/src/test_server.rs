//! A real socket for tests of what Proton answers, so requests pass through the real HTTP client.

use parking_lot::Mutex;
use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

pub struct Server {
    pub base: String,
    /// The paths asked for, in order.
    pub seen: Arc<Mutex<Vec<String>>>,
}

fn read_request(stream: &mut TcpStream) -> String {
    let (mut request, mut chunk) = (Vec::new(), [0; 4096]);
    while let Ok(read) = stream.read(&mut chunk) {
        request.extend_from_slice(chunk.get(..read).unwrap_or_default());
        let text = String::from_utf8_lossy(&request);
        let Some((head, body)) = text.split_once("\r\n\r\n") else {
            if read == 0 {
                break;
            }
            continue;
        };
        let length = head.lines().find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length").then(|| value.trim().parse::<usize>().ok())?
        });
        if read == 0 || body.len() >= length.unwrap_or(0) {
            break;
        }
    }
    String::from_utf8_lossy(&request).split_whitespace().nth(1).unwrap_or_default().to_string()
}

/// `answer` gets the path and how many requests came before. `None` closes the server, and from
/// then on nothing answers, as when a network drops.
pub fn serve(answer: impl Fn(&str, usize) -> Option<(u16, &'static str)> + Send + 'static) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    std::thread::spawn(move || {
        for (index, stream) in listener.incoming().enumerate() {
            let mut stream = stream.unwrap();
            let path = read_request(&mut stream);
            log.lock().push(path.clone());
            let Some((status, body)) = answer(&path, index) else { return };
            let length = body.len();
            let reply = format!(
                "HTTP/1.1 {status} X\r\nRetry-After: 0\r\nConnection: close\r\n\
                 Content-Length: {length}\r\n\r\n{body}"
            );
            let _ = stream.write_all(reply.as_bytes());
        }
    });
    Server { base, seen }
}

/// A server anyone could set up on the network: valid TLS, for the right name, from no one trusted.
/// The flag is raised if it ever gets to read a request.
pub fn false_server(name: &str) -> (SocketAddr, Arc<AtomicBool>) {
    let key = rcgen::KeyPair::generate().unwrap();
    let certificate = rcgen::CertificateParams::new(vec![name.to_string()]).unwrap().self_signed(&key).unwrap();
    let private = rustls::pki_types::PrivateKeyDer::try_from(key.serialize_der()).unwrap();
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(vec![certificate.der().clone()], private)
        .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let reached = Arc::new(AtomicBool::new(false));
    let flag = reached.clone();
    let config = Arc::new(config);
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut stream = stream;
            let Ok(mut session) = rustls::ServerConnection::new(config.clone()) else { continue };
            let mut tls = rustls::Stream::new(&mut session, &mut stream);
            if matches!(tls.read(&mut [0; 16]), Ok(read) if read > 0) {
                flag.store(true, Ordering::Relaxed);
            }
        }
    });
    (address, reached)
}

/// Answers in the order given, and then stops answering.
pub fn script(replies: &[(u16, &'static str)]) -> Server {
    let replies = replies.to_vec();
    serve(move |_, index| replies.get(index).copied())
}
