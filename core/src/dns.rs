//! Every name this client looks up goes to Quad9 over DNS-over-HTTPS, and nowhere else.
//!
//! There is no fallback to the system's resolver: that would hand the lookup back to whoever controls the network.
//! Quad9's own addresses are built in, since finding them would need DNS already.

use crate::{
    error::{Error, Result},
    tls,
};
use hickory_proto::{
    op::{Message, MessageType, OpCode, Query, ResponseCode},
    rr::{Name, RData, Record, RecordType},
};
use parking_lot::Mutex;
use reqwest::dns::{Addrs, Name as HostName, Resolve, Resolving};
use std::{
    collections::HashMap,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::Arc,
    time::{Duration, Instant},
};

const HOST: &str = "dns.quad9.net";
const ENDPOINT: &str = "https://dns.quad9.net/dns-query";
const MESSAGE_TYPE: &str = "application/dns-message";
const PORT: u16 = 443;
const ADDRESSES: [IpAddr; 4] = [
    IpAddr::V4(Ipv4Addr::new(9, 9, 9, 9)),
    IpAddr::V4(Ipv4Addr::new(149, 112, 112, 112)),
    IpAddr::V6(Ipv6Addr::new(0x2620, 0xfe, 0, 0, 0, 0, 0, 0xfe)),
    IpAddr::V6(Ipv6Addr::new(0x2620, 0xfe, 0, 0, 0, 0, 0, 0x9)),
];
const TIMEOUT: Duration = Duration::from_secs(10);
const MIN_CACHE_SECONDS: u32 = 30;
const MAX_CACHE_SECONDS: u32 = 300;

fn seconds_to_keep(records: &[Record]) -> u32 {
    let shortest = records.iter().map(|record| record.ttl).min().unwrap_or(MIN_CACHE_SECONDS);
    shortest.clamp(MIN_CACHE_SECONDS, MAX_CACHE_SECONDS)
}

fn encode_query(name: &str, kind: RecordType) -> Result<Vec<u8>> {
    let name = Name::from_utf8(name).map_err(|_| Error::InvalidAddress(name.to_string()))?;
    // Id 0, as RFC 8484 asks of DNS over HTTPS, so that answers can be cached.
    let mut message = Message::new(0, MessageType::Query, OpCode::Query);
    message.metadata.recursion_desired = true;
    message.add_query(Query::query(name, kind));
    message.to_vec().map_err(Error::other_from)
}

#[cfg(test)]
fn decode_answer(bytes: &[u8]) -> Result<Vec<Record>> {
    decode(bytes).map(|(_, records)| records)
}

/// A name that does not exist is an empty answer, and anything else that went wrong is an error.
fn decode(bytes: &[u8]) -> Result<(ResponseCode, Vec<Record>)> {
    let message = Message::from_vec(bytes).map_err(|_| Error::other("A broken DNS answer"))?;
    match message.metadata.response_code {
        code @ (ResponseCode::NoError | ResponseCode::NXDomain) => Ok((code, message.answers)),
        _ => Err(Error::Network("the DNS lookup failed".to_string())),
    }
}

/// Proton's own hosts may be named in the journal. Any other name is a recipient's domain or the
/// person's own, and says who they write to.
fn is_protons(host: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    ["proton.me", "protonmail.ch", "protonmail.com"]
        .iter()
        .any(|own| host == *own || host.ends_with(&format!(".{own}")))
}

/// What an answer held, as the journal may tell it: every record of Proton's, and of anyone else's
/// only how many of each kind, since an MX or TXT record can name a company.
fn summary(records: &[Record], protons: bool) -> String {
    if records.is_empty() {
        return "nothing".to_string();
    }
    if protons {
        return records
            .iter()
            .map(|record| format!("{} {} (ttl {} s)", record.record_type(), record.data, record.ttl))
            .collect::<Vec<_>>()
            .join(", ");
    }
    let mut kinds: Vec<(RecordType, usize)> = Vec::new();
    for record in records {
        match kinds.iter_mut().find(|(kind, _)| *kind == record.record_type()) {
            Some((_, count)) => *count += 1,
            None => kinds.push((record.record_type(), 1)),
        }
    }
    kinds.iter().map(|(kind, count)| format!("{count} {kind}")).collect::<Vec<_>>().join(", ")
}

/// Mail goes to the MX hosts, or with none to the domain itself. An MX of "." says no mail at all.
fn takes_mail(hosts: &[(u16, String)], has_address: bool) -> bool {
    if hosts.is_empty() { has_address } else { hosts.iter().any(|(_, host)| !host.trim_end_matches('.').is_empty()) }
}

fn addresses(records: &[Record]) -> Vec<IpAddr> {
    records
        .iter()
        .filter_map(|record| match &record.data {
            RData::A(address) => Some(IpAddr::V4(address.0)),
            RData::AAAA(address) => Some(IpAddr::V6(address.0)),
            _ => None,
        })
        .collect()
}

fn texts(records: &[Record]) -> Vec<String> {
    records
        .iter()
        .filter_map(|record| match &record.data {
            RData::TXT(text) => {
                let parts = text.txt_data.iter().flat_map(|part| part.iter().copied()).collect::<Vec<u8>>();
                Some(String::from_utf8_lossy(&parts).into_owned())
            }
            _ => None,
        })
        .collect()
}

fn mail_hosts(records: &[Record]) -> Vec<(u16, String)> {
    records
        .iter()
        .filter_map(|record| match &record.data {
            RData::MX(mx) => Some((mx.preference, mx.exchange.to_ascii())),
            _ => None,
        })
        .collect()
}

fn aliases(records: &[Record]) -> Vec<String> {
    records
        .iter()
        .filter_map(|record| match &record.data {
            RData::CNAME(alias) => Some(alias.0.to_ascii()),
            _ => None,
        })
        .collect()
}

type Cache = Arc<Mutex<HashMap<String, (Vec<IpAddr>, Instant)>>>;

/// Refuses every name. Under Tor nothing is looked up on this machine, and a connection that tried would fail here
/// instead of going out.
pub struct NoLookups;

impl Resolve for NoLookups {
    fn resolve(&self, _name: HostName) -> Resolving {
        log::error!("a name was to be looked up outside Tor, and was not");
        Box::pin(async { Err(Box::new(Error::Network("no lookups outside Tor".to_string())).into()) })
    }
}

pub struct Quad9 {
    http: crate::tor::Connections,
    cache: Cache,
}

impl Quad9 {
    pub fn new() -> Result<Self> {
        Self::at(&ADDRESSES.map(|address| SocketAddr::new(address, PORT)))
    }

    fn at(addresses: &[SocketAddr]) -> Result<Self> {
        let tls = tls::quad9_config().map_err(Error::other_from)?;
        let http = reqwest::Client::builder()
            .use_preconfigured_tls(tls)
            .resolve_to_addrs(HOST, addresses)
            .https_only(true)
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(TIMEOUT)
            .build()?;
        Ok(Self { http: crate::tor::Connections::fixed(http), cache: Arc::default() })
    }

    /// Quad9 through Tor: its built-in address goes into Tor as a number, on circuits of its own, and no name is
    /// looked up anywhere on the way.
    pub fn through(tor: &crate::tor::Tor) -> Result<Self> {
        let tls = tls::quad9_config().map_err(Error::other_from)?;
        let addresses = ADDRESSES.map(|address| SocketAddr::new(address, PORT));
        let proxy = tor.proxy_for_quad9();
        let http = crate::tor::Connections::following(tor, move || {
            Ok(reqwest::Client::builder()
                .use_preconfigured_tls(tls.clone())
                .resolve_to_addrs(HOST, &addresses)
                .dns_resolver(Arc::new(NoLookups))
                .proxy(reqwest::Proxy::all(&proxy)?)
                .https_only(true)
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(crate::tor::CONNECT_TIMEOUT)
                .timeout(crate::tor::CONNECT_TIMEOUT + TIMEOUT)
                .build()?)
        })?;
        Ok(Self { http, cache: Arc::default() })
    }

    async fn ask(http: &reqwest::Client, name: &str, kind: RecordType) -> Result<Vec<Record>> {
        let protons = is_protons(name);
        let asked = if protons { name } else { "a name of the person's" };
        log::debug!("Quad9 <- {kind} {asked}");
        let started = Instant::now();
        let sent = http
            .post(ENDPOINT)
            .header("content-type", MESSAGE_TYPE)
            .header("accept", MESSAGE_TYPE)
            .body(encode_query(name, kind)?)
            .send()
            .await
            .map_err(Error::from);
        let response = sent.inspect_err(|error| log::warn!("Quad9 -> {kind} {asked}: no answer, {}", error.kind()))?;
        if !response.status().is_success() {
            log::warn!("Quad9 -> {kind} {asked}: HTTP {}", response.status().as_u16());
            return Err(Error::Network("the DNS lookup failed".to_string()));
        }
        let decoded = decode(&response.bytes().await?);
        let elapsed = started.elapsed().as_millis();
        match &decoded {
            Ok((code, records)) => {
                log::debug!("Quad9 -> {kind} {asked}: {code} in {elapsed} ms, {}", summary(records, protons));
            }
            Err(error) => log::warn!("Quad9 -> {kind} {asked}: {} in {elapsed} ms", error.kind()),
        }
        decoded.map(|(_, records)| records)
    }

    /// A real question, so that the time is that of a real answer.
    pub async fn ping(&self) -> Result<()> {
        Self::ask(&self.http.client(), "mail-api.proton.me", RecordType::A).await.map(drop)
    }

    pub async fn txt(&self, name: &str) -> Result<Vec<String>> {
        Ok(texts(&Self::ask(&self.http.client(), name, RecordType::TXT).await?))
    }

    pub async fn mx(&self, name: &str) -> Result<Vec<(u16, String)>> {
        Ok(mail_hosts(&Self::ask(&self.http.client(), name, RecordType::MX).await?))
    }

    /// Whether a domain can receive mail at all. A failed lookup is an error, not a no.
    pub async fn accepts_mail(&self, name: &str) -> Result<bool> {
        let hosts = self.mx(name).await?;
        let has_address = hosts.is_empty() && !Self::ask(&self.http.client(), name, RecordType::A).await?.is_empty();
        Ok(takes_mail(&hosts, has_address))
    }

    pub async fn cname(&self, name: &str) -> Result<Vec<String>> {
        Ok(aliases(&Self::ask(&self.http.client(), name, RecordType::CNAME).await?))
    }

    async fn addresses_of(http: reqwest::Client, cache: Cache, name: String) -> Result<Vec<IpAddr>> {
        if let Some((known, until)) = cache.lock().get(&name)
            && *until > Instant::now()
        {
            let left = until.saturating_duration_since(Instant::now()).as_secs();
            log::debug!("Quad9 not asked: the answer is kept for {left} s more");
            return Ok(known.clone());
        }
        let (v4, v6) =
            futures::join!(Self::ask(&http, &name, RecordType::A), Self::ask(&http, &name, RecordType::AAAA),);
        let records: Vec<Record> = match (v4, v6) {
            (Ok(mut v4), Ok(v6)) => {
                v4.extend(v6);
                v4
            }
            (Ok(only), Err(_)) | (Err(_), Ok(only)) => only,
            (Err(error), Err(_)) => return Err(error),
        };
        let found = addresses(&records);
        let until = Instant::now() + Duration::from_secs(u64::from(seconds_to_keep(&records)));
        cache.lock().insert(name, (found.clone(), until));
        Ok(found)
    }
}

impl Resolve for Quad9 {
    fn resolve(&self, name: HostName) -> Resolving {
        let (http, cache) = (self.http.client(), self.cache.clone());
        Box::pin(async move {
            let found = Self::addresses_of(http, cache, name.as_str().to_string()).await.map_err(Box::new)?;
            if found.is_empty() {
                return Err(Box::new(Error::Network("the name has no address".to_string())).into());
            }
            let addresses: Addrs = Box::new(found.into_iter().map(|address| SocketAddr::new(address, 0)));
            Ok(addresses)
        })
    }
}

#[cfg(test)]
mod tests;
