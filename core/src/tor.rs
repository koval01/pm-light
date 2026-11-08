//! Tor, inside the program, by the Tor Project's own Rust implementation (arti).
//!
//! When the person chooses Tor, nothing of the app's goes to the network another way: Proton's API, Quad9 and Proton's
//! verification page all pass through a SOCKS5 door that only this program opens, on the loopback interface, to
//! Proton's and Quad9's hosts alone. If Tor cannot connect, the app waits and says so; it never falls back to a direct
//! connection, which would show the very address Tor was chosen to hide.
//!
//! Proton's onion site is the default route: the connection never leaves Tor, and no exit relay sees it. The other
//! route leaves Tor at an exit for `mail-api.proton.me`, where Proton's pinned keys still guard the session.
//!
//! What Tor keeps on disk, the guards above all, stays between starts: choosing new guards each time is what would
//! make a person easier to follow.

mod connections;
pub mod slow;
mod socks;

pub use connections::Connections;

use socks::{Entry, Purpose};

#[cfg(test)]
mod tests;

use crate::error::{Error, Result};
use arti_client::{HasKind, TorClient, TorClientConfig, config::CfgPath};
use futures::StreamExt;
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use std::{
    net::SocketAddr,
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};
use tor_geoip::HasCountryCode;
use tor_linkspec::HasAddrs;
use tor_proto::client::stream::ClientStreamCtrl;
use tor_rtcompat::PreferredRuntime;

/// Proton's onion site, as its own web client names it (`packages/shared/lib/constants.ts`).
pub const ONION_HOST: &str = "protonmailrmez3lotccipshtkleegetolb73fuirgj7r4o4vfu7ozyd.onion";
/// The API beside the onion site, by the rule the web client follows: `mail` becomes `mail-api`.
pub const ONION_API: &str = "https://mail-api.protonmailrmez3lotccipshtkleegetolb73fuirgj7r4o4vfu7ozyd.onion";
/// A first connection to an onion site builds a rendezvous and took up to 45 s when measured.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(90);
/// How long a stream waits before asking again while Tor is not ready to carry it.
const NOT_READY_PAUSE: Duration = Duration::from_millis(250);

/// How Proton is reached through Tor.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Route {
    /// Proton's onion site: the connection never leaves Tor.
    #[default]
    Onion,
    /// Out of an exit relay to `mail-api.proton.me`.
    Exit,
}

impl Route {
    #[must_use]
    pub const fn api(self) -> &'static str {
        match self {
            Self::Onion => ONION_API,
            Self::Exit => crate::client::PROTON_API,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TorState {
    #[default]
    Starting,
    Ready,
    /// Tor could not start. Nothing goes out until it does.
    Failed,
}

/// What the window shows of Tor.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct TorStatus {
    pub state: TorState,
    /// How far the start has come, out of 100.
    pub percent: u8,
    /// The countries of the relays on the last circuit Proton was reached over, first hop first, as two letters,
    /// `??` where the relay's country is not known. Proton's own hops to an onion site are its, and not shown.
    pub circuit: Vec<String>,
    /// The same relays with their addresses, for the connection page alone: they never go into the journal.
    pub hops: Vec<Hop>,
}

/// One relay of a circuit, as Tor Browser shows it: its country and its address.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Hop {
    pub country: String,
    pub address: String,
}

type StatusListener = Box<dyn Fn(&TorStatus) + Send + Sync>;

/// One Tor client for each purpose: they share the guards and the directory, and never a circuit.
struct Clients {
    proton: Arc<TorClient<PreferredRuntime>>,
    quad9: Arc<TorClient<PreferredRuntime>>,
    page: Arc<TorClient<PreferredRuntime>>,
}

impl Clients {
    fn from(base: &Arc<TorClient<PreferredRuntime>>) -> Self {
        Self { proton: base.isolated_client(), quad9: base.isolated_client(), page: base.isolated_client() }
    }

    fn of(&self, purpose: Purpose) -> Arc<TorClient<PreferredRuntime>> {
        match purpose {
            Purpose::Proton => self.proton.clone(),
            Purpose::Quad9 => self.quad9.clone(),
            Purpose::Page => self.page.clone(),
        }
    }
}

pub struct Tor {
    base: Arc<TorClient<PreferredRuntime>>,
    clients: RwLock<Clients>,
    status: Mutex<TorStatus>,
    listener: Mutex<Option<StatusListener>>,
    /// The door for the app's own clients, which ask with a name and password for each purpose.
    door: SocketAddr,
    proton_secret: (String, String),
    quad9_secret: (String, String),
    /// A door without a password, for a webview, which cannot give one. Open only while one needs it.
    page_door: Mutex<Option<(SocketAddr, tokio::task::JoinHandle<()>)>>,
    /// Counts the times every open stream was cut, so that a connection kept alive cannot carry on over a circuit
    /// that was given up.
    cuts: tokio::sync::watch::Sender<u64>,
    /// How quickly Proton answers through Tor, for building new circuits when the one to Proton is slow.
    slow: Mutex<slow::Watch>,
}

impl Tor {
    /// Starts Tor in the background and opens the door to it. Requests that come before Tor is ready wait for it.
    pub async fn start(folder: &Path) -> Result<Arc<Self>> {
        let mut builder = TorClientConfig::builder();
        builder
            .storage()
            .state_dir(CfgPath::new_literal(folder.join("state")))
            .cache_dir(CfgPath::new_literal(folder.join("cache")));
        let config = builder.build().map_err(|error| Error::other(format!("Tor's settings: {error}")))?;
        let client = TorClient::builder()
            .config(config)
            .bootstrap_behavior(arti_client::BootstrapBehavior::OnDemand)
            .create_unbootstrapped()
            .map_err(|error| Error::other(format!("Tor could not start: {}", error.kind())))?;
        let (proton_secret, quad9_secret) = ((random_word(), random_word()), (random_word(), random_word()));
        let (door, accepting) = socks::listen().await?;
        let entry = Entry::Passwords(vec![
            (proton_secret.0.clone(), proton_secret.1.clone(), Purpose::Proton),
            (quad9_secret.0.clone(), quad9_secret.1.clone(), Purpose::Quad9),
        ]);
        let tor = Arc::new(Self {
            clients: RwLock::new(Clients::from(&client)),
            base: client,
            status: Mutex::new(TorStatus::default()),
            listener: Mutex::new(None),
            door,
            proton_secret,
            quad9_secret,
            page_door: Mutex::new(None),
            cuts: tokio::sync::watch::Sender::new(0),
            slow: Mutex::new(slow::Watch::default()),
        });
        tokio::spawn(socks::serve(accepting, Arc::downgrade(&tor), entry));
        tokio::spawn(Self::follow_the_start(Arc::downgrade(&tor)));
        log::info!("Tor starting");
        Ok(tor)
    }

    async fn follow_the_start(tor: std::sync::Weak<Self>) {
        let Some(strong) = tor.upgrade() else { return };
        let client = strong.base.clone();
        drop(strong);
        let mut events = client.bootstrap_events();
        let started = Instant::now();
        let watching = {
            let tor = tor.clone();
            async move {
                let mut last = 0_u8;
                while let Some(event) = events.next().await {
                    let Some(tor) = tor.upgrade() else { return };
                    let percent = percent_of(event.as_frac());
                    if percent >= last.saturating_add(10) || event.ready_for_traffic() {
                        log::info!("Tor start: {percent}%");
                        last = percent;
                    }
                    tor.update(|status| {
                        status.percent = percent;
                        if event.ready_for_traffic() {
                            status.state = TorState::Ready;
                        }
                    });
                }
            }
        };
        tokio::spawn(watching);
        match client.bootstrap().await {
            Ok(()) => {
                log::info!("Tor is ready in {} ms", started.elapsed().as_millis());
                if let Some(tor) = tor.upgrade() {
                    tor.update(|status| {
                        status.state = TorState::Ready;
                        status.percent = 100;
                    });
                }
            }
            Err(error) => {
                log::error!("Tor did not start: {}", error.kind());
                if let Some(tor) = tor.upgrade() {
                    tor.update(|status| status.state = TorState::Failed);
                }
            }
        }
    }

    fn update(&self, change: impl FnOnce(&mut TorStatus)) {
        let status = {
            let mut status = self.status.lock();
            let before = status.clone();
            change(&mut status);
            if *status == before {
                return;
            }
            status.clone()
        };
        if let Some(listener) = self.listener.lock().as_ref() {
            listener(&status);
        }
    }

    pub fn on_status(&self, listener: StatusListener) {
        let status = self.status();
        listener(&status);
        *self.listener.lock() = Some(listener);
    }

    #[must_use]
    pub fn status(&self) -> TorStatus {
        self.status.lock().clone()
    }

    /// For the client of Proton's API: names go through Tor, and are never looked up here.
    #[must_use]
    pub fn proxy_for_proton(&self) -> String {
        proxy_url(Names::InTor, &self.proton_secret, self.door)
    }

    /// For Quad9, whose addresses are built in: the address goes into Tor, and no name is looked up anywhere.
    #[must_use]
    pub fn proxy_for_quad9(&self) -> String {
        proxy_url(Names::None, &self.quad9_secret, self.door)
    }

    /// A door for a webview, which cannot give a password. It lets through Proton's hosts only, and closes when
    /// the page does.
    pub fn open_page_door(self: &Arc<Self>) -> Result<SocketAddr> {
        if let Some((address, _)) = self.page_door.lock().as_ref() {
            return Ok(*address);
        }
        let (address, accepting) = socks::listen_now()?;
        let serving = tokio::spawn(socks::serve(accepting, Arc::downgrade(self), Entry::Open(Purpose::Page)));
        *self.page_door.lock() = Some((address, serving));
        log::info!("a door for a page opened");
        Ok(address)
    }

    pub fn close_page_door(&self) {
        let door = self.page_door.lock().take();
        if let Some((_, serving)) = door {
            serving.abort();
            log::info!("the door for a page closed");
        }
    }

    /// New circuits for everything from now on, as Tor Browser's New Identity: the old ones are no longer used.
    ///
    /// Every stream open now is cut as well. An HTTP client keeps its connections alive and would otherwise go on
    /// sending over the old circuit for as long as it is busy, which is what the person asked to leave.
    pub fn new_circuits(&self) {
        *self.clients.write() = Clients::from(&self.base);
        self.cuts.send_modify(|cuts| *cuts = cuts.wrapping_add(1));
        self.slow.lock().renewed();
        self.update(|status| {
            status.circuit.clear();
            status.hops.clear();
        });
        log::info!("Tor: new circuits from now on, the open streams cut");
    }

    /// Proton answered through Tor in `took`. True when the answers have been slow, and it is time to see whether the
    /// circuit is to blame.
    pub fn heard_back(&self, took: Duration) -> bool {
        self.slow.lock().heard(took, Instant::now())
    }

    /// Quad9 was asked over its own circuit to tell a slow circuit from a slow network: new circuits for the first.
    pub fn judge(&self, quad9: Option<Duration>) {
        let middle = self.slow.lock().middle().as_millis();
        if slow::network_works(quad9) {
            log::info!(
                "Tor: Proton answered in {middle} ms in the middle while Quad9 answered in {} ms over its own circuit: new circuits",
                quad9.map_or(0, |took| took.as_millis())
            );
            self.new_circuits();
        } else {
            log::info!("Tor: Proton answered in {middle} ms in the middle, and Quad9 was slow too: the network is slow, not the circuit");
        }
    }

    /// Tells a stream when it must end because its circuit was given up.
    pub(crate) fn cut_signal(&self) -> tokio::sync::watch::Receiver<u64> {
        self.cuts.subscribe()
    }

    /// A stream through Tor to an allowed host, and the countries of its circuit noted.
    async fn connect(&self, purpose: Purpose, host: &str, port: u16) -> Result<arti_client::DataStream> {
        let client = self.clients.read().of(purpose);
        let started = Instant::now();
        let stream = patiently(CONNECT_TIMEOUT, NOT_READY_PAUSE, || client.connect((host, port))).await?;
        let hops = hops_of(&client, &stream);
        let countries: Vec<String> = hops.iter().map(|hop| hop.country.clone()).collect();
        log::info!(
            "Tor: a stream to {} in {} ms over {}",
            host_for_the_journal(host),
            started.elapsed().as_millis(),
            countries.join(" > ")
        );
        // What the window shows is the way to Proton: Quad9's and the page's circuits are their own.
        if purpose == Purpose::Proton {
            self.update(|status| {
                status.circuit = countries;
                status.hops = hops;
            });
        }
        Ok(stream)
    }
}

impl Drop for Tor {
    fn drop(&mut self) {
        self.close_page_door();
    }
}

/// A stream, waited for while Tor cannot carry it yet, for at most `limit` in all.
///
/// Tor may call itself started from the directory it kept, and still refuse a circuit until a newer one arrives: the
/// app asks at once, and was put on the waiting screen by that refusal. Not being ready is worth waiting for; any
/// other refusal is answered at once.
async fn patiently<T, E, F>(limit: Duration, pause: Duration, mut attempt: impl FnMut() -> F) -> Result<T>
where
    E: HasKind,
    F: Future<Output = std::result::Result<T, E>>,
{
    let deadline = Instant::now() + limit;
    let mut waited = false;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        let error = match tokio::time::timeout(left, attempt()).await {
            Err(_) => return Err(Error::Network("Tor took too long to connect".to_string())),
            Ok(Ok(stream)) => return Ok(stream),
            Ok(Err(error)) => error,
        };
        let not_ready = error.kind() == arti_client::ErrorKind::BootstrapRequired;
        if !not_ready || deadline.saturating_duration_since(Instant::now()) <= pause {
            log::warn!("Tor: no stream: {}", error.kind());
            return Err(Error::Network("Tor could not connect".to_string()));
        }
        if !waited {
            log::info!("Tor: not ready to carry a stream yet, it waits");
            waited = true;
        }
        tokio::time::sleep(pause).await;
    }
}

/// The relays a stream goes over, as far as this side of the circuit knows them: an onion site's own hops are its.
fn hops_of(client: &TorClient<PreferredRuntime>, stream: &arti_client::DataStream) -> Vec<Hop> {
    let Some(tunnel) = stream.client_stream_ctrl().and_then(|ctrl| ctrl.tunnel()) else { return Vec::new() };
    let directory = client.dirmgr().ok().and_then(|provider| provider.netdir(tor_netdir::Timeliness::Timely).ok());
    let mut hops = Vec::new();
    for path in tunnel.all_paths() {
        for hop in path.hops() {
            // A hop with no relay behind it is the onion service's side of the rendezvous.
            let Some(relay) = hop.as_chan_target() else { continue };
            let ip = relay.addrs().next().map(|address| address.ip());
            let country = directory
                .as_ref()
                .and_then(|directory| directory.by_ids(relay))
                .and_then(|relay| relay.country_code())
                .map(|code| code.as_ref().to_string())
                .or_else(|| country_by_address(ip))
                .unwrap_or_else(|| "??".to_string());
            let address = ip.map_or_else(String::new, |ip| ip.to_string());
            hops.push(Hop { country, address });
        }
    }
    hops
}

/// A relay's country by its address, in the database Tor carries: for a relay the directory gives none for.
fn country_by_address(ip: Option<std::net::IpAddr>) -> Option<String> {
    let ip = ip?;
    tor_geoip::GeoipDb::new_embedded().lookup_country_code(ip).map(|code| code.as_ref().to_string())
}

/// Proton's hosts by their name, and anything else not at all.
fn host_for_the_journal(host: &str) -> &str {
    if socks::is_allowed(host) { host } else { "another host" }
}

fn percent_of(fraction: f32) -> u8 {
    #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "clamped to 0..=100 just before")]
    let percent = (fraction * 100.0).round().clamp(0.0, 100.0) as u8;
    percent
}

/// Where a name in a request is turned into an address.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Names {
    /// At the far end of the circuit: `socks5h`. With plain `socks5`, reqwest would ask the system's resolver first,
    /// and the name of every host would leave the computer outside Tor.
    InTor,
    /// The client gives addresses only, and its resolver refuses every name.
    None,
}

fn proxy_url(names: Names, (name, password): &(String, String), door: SocketAddr) -> String {
    let scheme = match names {
        Names::InTor => "socks5h",
        Names::None => "socks5",
    };
    format!("{scheme}://{name}:{password}@{door}")
}

fn random_word() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    crate::hex(&bytes)
}
