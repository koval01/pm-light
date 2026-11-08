//! The door to Tor: a SOCKS5 server (RFC 1928, with RFC 1929's name and password) on the loopback interface, which
//! hands each connection to arti. It opens only for Proton's hosts and Quad9's addresses, so that nothing else on
//! the machine can use it as a way out, and it never looks a name up itself: the name goes into Tor as it came.
//!
//! What arrives here comes from another process and is read as hostile: every length is checked, every wait is
//! bounded, and anything unexpected closes the connection.

use super::Tor;
use crate::error::{Error, Result};
use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::Weak,
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

const VERSION: u8 = 5;
const NO_AUTHENTICATION: u8 = 0;
const PASSWORD: u8 = 2;
const NO_ACCEPTABLE: u8 = 0xff;
const PASSWORD_VERSION: u8 = 1;
const CONNECT: u8 = 1;
const IPV4: u8 = 1;
const DOMAIN: u8 = 3;
const IPV6: u8 = 4;
const SUCCEEDED: u8 = 0;
const FAILURE: u8 = 1;
const NOT_ALLOWED: u8 = 2;
const HOST_UNREACHABLE: u8 = 4;
const COMMAND_NOT_SUPPORTED: u8 = 7;
const ADDRESS_NOT_SUPPORTED: u8 = 8;
/// A client that does not finish asking in this time is not one of ours.
const HANDSHAKE: Duration = Duration::from_secs(10);
/// Quad9's resolver, the only address that may be asked for by number.
const QUAD9: [IpAddr; 4] = [
    IpAddr::V4(Ipv4Addr::new(9, 9, 9, 9)),
    IpAddr::V4(Ipv4Addr::new(149, 112, 112, 112)),
    IpAddr::V6(Ipv6Addr::new(0x2620, 0xfe, 0, 0, 0, 0, 0, 0xfe)),
    IpAddr::V6(Ipv6Addr::new(0x2620, 0xfe, 0, 0, 0, 0, 0, 0x9)),
];

/// Who asks, told apart by the name it gives. Each has circuits of its own, as Tor's `IsolateSOCKSAuth` keeps
/// them, so that what Proton sees and what Quad9 sees cannot be joined by the circuit they came over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Purpose {
    Proton,
    Quad9,
    /// Proton's verification page, in a webview.
    Page,
}

/// How a door lets clients in.
#[derive(Clone, Debug)]
pub(super) enum Entry {
    /// By name and password, the purpose following from the name.
    Passwords(Vec<(String, String, Purpose)>),
    /// Without a password, for one purpose: a webview cannot give one.
    Open(Purpose),
}

/// Where a connection was asked to go.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Target {
    Name(String),
    Address(IpAddr),
}

/// Proton's own hosts, and its onion site with whatever is in front of it.
pub(super) fn is_allowed(host: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    ["proton.me", "protonmail.ch", "protonmail.com", super::ONION_HOST]
        .iter()
        .any(|own| host == *own || host.ends_with(&format!(".{own}")))
}

pub(super) fn may_go(purpose: Purpose, target: &Target, port: u16) -> bool {
    port == 443
        && match (purpose, target) {
            (Purpose::Proton | Purpose::Page, Target::Name(host)) => is_allowed(host),
            (Purpose::Quad9, Target::Address(address)) => QUAD9.contains(address),
            _ => false,
        }
}

pub(super) async fn listen() -> Result<(SocketAddr, TcpListener)> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
    let address = listener.local_addr()?;
    Ok((address, listener))
}

/// The same, from the window's thread, which cannot wait: it needs a Tokio runtime entered, as the app's is.
pub(super) fn listen_now() -> Result<(SocketAddr, TcpListener)> {
    let listener = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    listener.set_nonblocking(true)?;
    let address = listener.local_addr()?;
    Ok((address, TcpListener::from_std(listener)?))
}

/// Serves until the listener's task is dropped.
pub(super) async fn serve(listener: TcpListener, tor: Weak<Tor>, entry: Entry) {
    loop {
        let Ok((stream, peer)) = listener.accept().await else { continue };
        // Only this machine, and the bind already says so; this is the second look.
        if !peer.ip().is_loopback() {
            continue;
        }
        let (tor, entry) = (tor.clone(), entry.clone());
        tokio::spawn(async move {
            if let Err(error) = handle(stream, &tor, &entry).await {
                log::debug!("Tor door: a connection ended: {}", error.kind());
            }
        });
    }
}

pub(super) async fn handle(mut stream: TcpStream, tor: &Weak<Tor>, entry: &Entry) -> Result<()> {
    let asked = tokio::time::timeout(HANDSHAKE, handshake(&mut stream, entry))
        .await
        .map_err(|_| Error::Network("too slow".to_string()))??;
    let Some((purpose, target, port)) = asked else { return Ok(()) };
    if !may_go(purpose, &target, port) {
        reply(&mut stream, NOT_ALLOWED).await?;
        log::warn!("Tor door: a connection to a host that is not Proton's was refused");
        return Ok(());
    }
    let Some(tor) = tor.upgrade() else {
        reply(&mut stream, FAILURE).await?;
        return Ok(());
    };
    let host = match &target {
        Target::Name(name) => name.clone(),
        Target::Address(address) => address.to_string(),
    };
    let mut onward = match tor.connect(purpose, &host, port).await {
        Ok(onward) => onward,
        Err(error) => {
            reply(&mut stream, HOST_UNREACHABLE).await?;
            return Err(error);
        }
    };
    // Taken with the stream: a circuit given up after this point ends the stream too.
    let mut cut = tor.cut_signal();
    cut.mark_unchanged();
    drop(tor);
    reply(&mut stream, SUCCEEDED).await?;
    tokio::select! {
        copied = tokio::io::copy_bidirectional(&mut stream, &mut onward) => {
            copied?;
        }
        _ = cut.changed() => {
            log::debug!("Tor door: a stream cut with its circuit");
        }
    }
    Ok(())
}

/// The greeting, the password if one is asked for, and the request. `None` when the client was turned away.
pub(super) async fn handshake(stream: &mut TcpStream, entry: &Entry) -> Result<Option<(Purpose, Target, u16)>> {
    let [version, count] = read_array::<2>(stream).await?;
    if version != VERSION {
        return Ok(None);
    }
    let methods = read_vec(stream, usize::from(count)).await?;
    let wanted = if matches!(entry, Entry::Passwords(_)) { PASSWORD } else { NO_AUTHENTICATION };
    if !methods.contains(&wanted) {
        stream.write_all(&[VERSION, NO_ACCEPTABLE]).await?;
        return Ok(None);
    }
    stream.write_all(&[VERSION, wanted]).await?;
    let purpose = match entry {
        Entry::Open(purpose) => *purpose,
        Entry::Passwords(known) => {
            let [password_version, name_length] = read_array::<2>(stream).await?;
            let name = read_vec(stream, usize::from(name_length)).await?;
            let [password_length] = read_array::<1>(stream).await?;
            let password = read_vec(stream, usize::from(password_length)).await?;
            // Every pair is compared whole, so that the time says nothing of which one came close.
            let mut found = None;
            for (known_name, known_password, purpose) in known {
                let right = (password_version == PASSWORD_VERSION)
                    & same(&name, known_name.as_bytes())
                    & same(&password, known_password.as_bytes());
                if right {
                    found = Some(*purpose);
                }
            }
            stream.write_all(&[PASSWORD_VERSION, u8::from(found.is_none())]).await?;
            let Some(purpose) = found else {
                log::warn!("Tor door: a wrong password");
                return Ok(None);
            };
            purpose
        }
    };
    let [version, command, _reserved, kind] = read_array::<4>(stream).await?;
    if version != VERSION {
        return Ok(None);
    }
    if command != CONNECT {
        reply(stream, COMMAND_NOT_SUPPORTED).await?;
        return Ok(None);
    }
    let target = match kind {
        IPV4 => Target::Address(IpAddr::V4(Ipv4Addr::from(read_array::<4>(stream).await?))),
        IPV6 => Target::Address(IpAddr::V6(Ipv6Addr::from(read_array::<16>(stream).await?))),
        DOMAIN => {
            let [length] = read_array::<1>(stream).await?;
            let name = read_vec(stream, usize::from(length)).await?;
            match String::from_utf8(name) {
                Ok(name) if !name.is_empty() => Target::Name(name),
                _ => {
                    reply(stream, ADDRESS_NOT_SUPPORTED).await?;
                    return Ok(None);
                }
            }
        }
        _ => {
            reply(stream, ADDRESS_NOT_SUPPORTED).await?;
            return Ok(None);
        }
    };
    let port = u16::from_be_bytes(read_array::<2>(stream).await?);
    Ok(Some((purpose, target, port)))
}

async fn read_array<const N: usize>(stream: &mut TcpStream) -> Result<[u8; N]> {
    let mut bytes = [0u8; N];
    stream.read_exact(&mut bytes).await?;
    Ok(bytes)
}

async fn read_vec(stream: &mut TcpStream, length: usize) -> Result<Vec<u8>> {
    let mut bytes = vec![0u8; length];
    stream.read_exact(&mut bytes).await?;
    Ok(bytes)
}

/// The same bytes, compared in a time that does not depend on where they differ.
pub(super) fn same(given: &[u8], expected: &[u8]) -> bool {
    if given.len() != expected.len() {
        return false;
    }
    given.iter().zip(expected).fold(0u8, |difference, (a, b)| difference | (a ^ b)) == 0
}

/// A reply with no bound address: the client has no use for one.
async fn reply(stream: &mut TcpStream, code: u8) -> Result<()> {
    stream.write_all(&[VERSION, code, 0, IPV4, 0, 0, 0, 0, 0, 0]).await?;
    Ok(())
}
