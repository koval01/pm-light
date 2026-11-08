//! An HTTP client that keeps its connections alive, and lets them go with the circuits they ran over.
//!
//! New circuits cut every stream through the door; a client still holding those connections would send its next
//! request into one of them and fail. So each request first asks whether streams were cut since this client was
//! made, and a client with fresh connections is made when they were.

use super::Tor;
use crate::error::Result;
use parking_lot::Mutex;
use tokio::sync::watch;

type Build = Box<dyn Fn() -> Result<reqwest::Client> + Send + Sync>;

pub struct Connections {
    build: Build,
    /// The client, and the count of cuts it was made after.
    current: Mutex<(u64, reqwest::Client)>,
    /// Tor's count of cuts; none when the connections do not go through Tor.
    cuts: Option<watch::Receiver<u64>>,
}

impl Connections {
    /// Connections with no circuits behind them: the client is kept for good.
    #[must_use]
    pub fn fixed(client: reqwest::Client) -> Self {
        let kept = client.clone();
        Self { build: Box::new(move || Ok(kept.clone())), current: Mutex::new((0, client)), cuts: None }
    }

    /// Connections through Tor, made again by `build` after each cut.
    pub fn following(tor: &Tor, build: impl Fn() -> Result<reqwest::Client> + Send + Sync + 'static) -> Result<Self> {
        let cuts = tor.cut_signal();
        let seen = *cuts.borrow();
        let client = build()?;
        Ok(Self { build: Box::new(build), current: Mutex::new((seen, client)), cuts: Some(cuts) })
    }

    /// The client to send with: one whose connections were all opened after the last cut.
    pub fn client(&self) -> reqwest::Client {
        let now = self.cuts.as_ref().map_or(0, |cuts| *cuts.borrow());
        let mut current = self.current.lock();
        if current.0 != now {
            match (self.build)() {
                Ok(fresh) => {
                    *current = (now, fresh);
                    log::debug!("Tor: the connections of the given-up circuits were let go");
                }
                // The old client's connections are cut already: its next request opens a new one, over a new circuit.
                Err(error) => log::error!("Tor: no fresh connections: {}", error.kind()),
            }
        }
        current.1.clone()
    }
}
