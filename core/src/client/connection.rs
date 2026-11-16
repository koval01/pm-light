//! How the client reaches Proton: an HTTP client that looks names up through Quad9 alone, or through a proxy that
//! looks up everything itself, with no redirects followed and dead connections found in seconds.

use super::user_agent;
use crate::{dns::Quad9, error::Result};
use std::{sync::Arc, time::Duration};

const KEEP_ALIVE: Duration = Duration::from_secs(25);
const KEEP_ALIVE_WAIT: Duration = Duration::from_secs(10);

/// With a proxy, every connection goes through it and nothing is looked up here; with none, Quad9 looks names up.
pub fn http_client_with(
    base: &str,
    tls: rustls::ClientConfig,
    proxy: Option<&str>,
    dns: &Arc<Quad9>,
) -> Result<reqwest::Client> {
    let builder = reqwest::Client::builder()
        .user_agent(user_agent())
        .timeout(Duration::from_secs(120))
        .use_preconfigured_tls(tls)
        .https_only(base.starts_with("https:"))
        // A dead connection is found in seconds, and not by every request stuck on it.
        .http2_keep_alive_interval(KEEP_ALIVE)
        .http2_keep_alive_timeout(KEEP_ALIVE_WAIT)
        .http2_keep_alive_while_idle(true)
        // Proton's API does not redirect. Following one would carry the session's headers along.
        .redirect(reqwest::redirect::Policy::none());
    let builder = match proxy {
        // A proxy from the environment would resolve names itself and sit in the connection.
        None => builder.no_proxy().dns_resolver(dns.clone()).connect_timeout(Duration::from_secs(20)),
        Some(proxy) => builder
            .proxy(reqwest::Proxy::all(proxy)?)
            .dns_resolver(Arc::new(crate::dns::NoLookups))
            .connect_timeout(crate::tor::CONNECT_TIMEOUT),
    };
    Ok(builder.build()?)
}
