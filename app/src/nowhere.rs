//! A door to nowhere. The app's own pages (a mail, the editor, previews, printouts) need no network: what they show
//! comes over protocols of the app's own. WebKit still connects by itself now and then, to the address of a clicked link
//! before the app has said no to it (`LinkPreconnect`), and such a connection went around Tor and Quad9 to a mail
//! tracker. So every connection of those pages is sent here, as to a proxy, and closed unread: it reaches no one, and
//! no name is looked up for it. The door listens on the loopback interface only, on a port the system picks, for as
//! long as the app runs.

use std::{
    io,
    net::{Ipv4Addr, SocketAddr, TcpListener},
    sync::{
        OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    thread,
};

static DOOR: OnceLock<Option<SocketAddr>> = OnceLock::new();
static TURNED_AWAY: AtomicU64 = AtomicU64::new(0);

/// Where the door is, opened the first time it is asked for. `None` if it could not open: the pages then go without
/// it, and the journal says so.
pub fn door() -> Option<SocketAddr> {
    *DOOR.get_or_init(|| match open() {
        Ok(at) => {
            log::info!("a door to nowhere opened for the app's pages");
            Some(at)
        }
        Err(error) => {
            log::warn!("no door to nowhere for the app's pages: {:?}", error.kind());
            None
        }
    })
}

/// How many connections the door has turned away since the app started.
pub fn turned_away() -> u64 {
    TURNED_AWAY.load(Ordering::Relaxed)
}

fn open() -> io::Result<SocketAddr> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    let at = listener.local_addr()?;
    thread::Builder::new().name("nowhere".to_string()).spawn(move || turn_away(&listener))?;
    Ok(at)
}

/// Closes every connection without reading or writing a byte.
fn turn_away(listener: &TcpListener) {
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        drop(stream);
        let count = TURNED_AWAY.fetch_add(1, Ordering::Relaxed) + 1;
        // WebView2's own browser reaches out on its own about once every two seconds: the journal says the 1st, the
        // 10th, the 100th and so on, and the long run's report gives the count.
        if worth_saying(count) {
            log::info!("a page's connection was turned away at the door to nowhere: {count} so far");
        }
    }
}

/// One, ten, a hundred and so on.
const fn worth_saying(count: u64) -> bool {
    count > 0 && 10_u64.pow(count.ilog10()) == count
}

#[cfg(test)]
mod tests;
