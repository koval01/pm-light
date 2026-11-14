//! The only code that talks to Proton: identity headers, retries, token refresh, batch results.

use crate::{
    dns::Quad9,
    error::{Error, HumanVerification, Result},
    journal::clean_path,
    request_log::RequestLog,
};
use parking_lot::Mutex;
use rand::RngCore;
use reqwest::{Method, Response, header::HeaderMap, multipart::Form};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use zeroize::{Zeroize, ZeroizeOnDrop};

mod answers;
pub use answers::first_batch_failure;
use answers::{check, refuses_session, retry_delay};

/// The host Proton's own apps use.
pub const PROTON_API: &str = "https://mail-api.proton.me";
/// Third-party clients say so: "web-mail@…" got the account flagged (2028), invented names fail (2064).
const APP_VERSION: &str = "Other";
const API_ACCEPT: &str = "application/vnd.protonmail.v1+json";
const VERSION: &str = env!("CARGO_PKG_VERSION");

const CODE_OK: i64 = 1000;
const CODE_BATCH: i64 = 1001;
const CODE_HUMAN_VERIFICATION: i64 = 9001;
const DEFAULT_RETRY_SECONDS: f64 = 10.0;
const MAX_RETRY_SECONDS: f64 = 30.0;
const REFRESH_PATH: &str = "/auth/v4/refresh";
/// Settings change rarely and are read on every visit to a tab: a kept answer is shown at once,
/// and past this age it is asked for again behind the scenes.
const KEPT_ANSWER_FRESH: Duration = Duration::from_secs(60);
const KEPT_ANSWER_OLDEST: Duration = Duration::from_secs(3600);
const KEPT_PATHS: [&str; 9] = [
    "/core/v4/settings",
    "/mail/v4/settings",
    "/payments/",
    "/auth/v4/sessions",
    "/core/v4/domains",
    "/core/v4/forwarding",
    "/mail/v4/filters",
    "/mail/v4/incomingdefaults",
    "/mail/v4/forwardings",
];
/// Everyday work on mail changes no setting.
const NOT_SETTINGS: [&str; 3] = ["/mail/v4/messages", "/mail/v4/conversations", "/mail/v4/attachments"];
const KEEP_ALIVE: Duration = Duration::from_secs(25);
const KEEP_ALIVE_WAIT: Duration = Duration::from_secs(10);
/// A refusal is asked again before anything is deleted, so that a slip on Proton's side costs a
/// moment and not a sign-in.
const REFUSAL_RECHECK: Duration = Duration::from_millis(if cfg!(test) { 200 } else { 10_000 });

#[must_use]
pub fn user_agent() -> String {
    format!(
        "proton-mail-light/{VERSION} (unofficial third-party Proton Mail client; \
         author: Yaroslav Koval; https://github.com/koval01)"
    )
}

/// Wiped when dropped: a token left behind in freed memory is as good as a stolen one.
#[derive(Clone, Deserialize, Serialize, PartialEq, Eq, Zeroize, ZeroizeOnDrop)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub uid: String,
    pub access_token: String,
    pub refresh_token: String,
    pub password_mode: u8,
}

/// Why a session is over. The screen that follows differs: nobody needs telling what they just did.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Ending {
    Chosen,
    /// Proton no longer accepts it.
    Refused,
}

pub enum SessionChange<'a> {
    Renewed(&'a Session),
    Ended(Ending),
}

type SessionListener = Box<dyn Fn(SessionChange<'_>) + Send + Sync>;

struct Verification {
    token: String,
    method: String,
}

/// How the client reaches Proton. Chosen once, before the first request, and never changed: a person who chose Tor
/// must not have a request go out directly because it started a moment before the change.
pub enum Way {
    Direct,
    Tor { tor: Arc<crate::tor::Tor>, route: crate::tor::Route },
}

struct Transport {
    http: crate::tor::Connections,
    dns: Arc<Quad9>,
    base: String,
    through_tor: bool,
    /// Tor, when the way goes through it: it hears how quickly Proton answers.
    tor: Option<Arc<crate::tor::Tor>>,
}

/// How long Quad9 is given to answer, when it is asked to tell a slow circuit from a slow network.
const QUAD9_PATIENCE: Duration = Duration::from_secs(8);

pub struct Client {
    transport: std::sync::OnceLock<Transport>,
    /// Where Proton is when it is reached directly; Tor's route has its own.
    direct_base: String,
    session: Mutex<Option<Session>>,
    verification: Mutex<Option<Verification>>,
    refreshing: tokio::sync::Mutex<()>,
    kept: Arc<Mutex<HashMap<String, KeptAnswer>>>,
    listener: Mutex<Option<SessionListener>>,
    refresh_forbidden: AtomicBool,
    /// A check borrowing the session asks only: anything but a read is refused here, before it leaves.
    changes_forbidden: AtomicBool,
    requests: std::sync::OnceLock<Arc<RequestLog>>,
    leaving: AtomicBool,
}

struct KeptAnswer {
    body: Kept,
    at: Instant,
}

/// A 403 to a setting means the plan does not have it, which changes as rarely as the setting
/// itself, so it is kept the same way instead of being asked again on every page that shows it.
#[derive(Clone)]
enum Kept {
    Answer(Value),
    NotInPlan { code: i64, message: String },
}

impl Kept {
    fn of(answer: &Result<Value>) -> Option<Self> {
        match answer {
            Ok(body) => Some(Self::Answer(body.clone())),
            Err(Error::Api { status: 403, code, message }) => {
                Some(Self::NotInPlan { code: *code, message: message.clone() })
            }
            Err(_) => None,
        }
    }

    fn answer(self) -> Result<Value> {
        match self {
            Self::Answer(body) => Ok(body),
            Self::NotInPlan { code, message } => Err(Error::Api { status: 403, code, message }),
        }
    }
}

#[derive(Default)]
struct Options {
    body: Option<Value>,
    no_auth: bool,
    batch: bool,
}

fn random_hex() -> String {
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    crate::hex(&bytes)
}

/// With a proxy, every connection goes through it and nothing is looked up here; with none, Quad9 looks names up.
/// Proton answered in `took`. Through Tor, slow answers lead to asking Quad9, over its own circuit, whether the network
/// works: if it does, the circuit to Proton is the slow part, and new circuits are built.
fn heard_back(transport: &Transport, took: Duration) {
    let Some(tor) = transport.tor.as_ref().filter(|tor| tor.heard_back(took)) else { return };
    let (tor, quad9) = (tor.clone(), transport.dns.clone());
    tokio::spawn(async move {
        let started = Instant::now();
        let answered = tokio::time::timeout(QUAD9_PATIENCE, quad9.ping()).await;
        let quad9_took = matches!(answered, Ok(Ok(()))).then(|| started.elapsed());
        tor.judge(quad9_took);
    });
}

pub(crate) fn http_client_with(
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

impl Client {
    /// Reached directly, as before Tor existed: for tests and for programs borrowing the app's session.
    pub fn new(base: &str) -> Result<Self> {
        let client = Self::undecided(base);
        client.decide(&Way::Direct)?;
        Ok(client)
    }

    /// No way to Proton yet: every request fails until `decide` is called.
    #[must_use]
    pub fn undecided(base: &str) -> Self {
        Self {
            transport: std::sync::OnceLock::new(),
            direct_base: base.trim_end_matches('/').to_string(),
            session: Mutex::new(None),
            verification: Mutex::new(None),
            refreshing: tokio::sync::Mutex::new(()),
            kept: Arc::default(),
            listener: Mutex::new(None),
            refresh_forbidden: AtomicBool::new(false),
            changes_forbidden: AtomicBool::new(false),
            requests: std::sync::OnceLock::new(),
            leaving: AtomicBool::new(false),
        }
    }

    /// Chooses how Proton is reached, once. A second choice is refused: changing it takes a new start of the app.
    pub fn decide(&self, way: &Way) -> Result<()> {
        if self.transport.get().is_some() {
            return Err(Error::other("the way to Proton is chosen already"));
        }
        let (base, tls, dns, through_tor) = match way {
            Way::Direct => (
                self.direct_base.clone(),
                crate::tls::pinned_config(crate::tls::PROTON_PINS).map_err(Error::other_from)?,
                Arc::new(Quad9::new()?),
                false,
            ),
            Way::Tor { tor, route } => (
                route.api().to_string(),
                match route {
                    crate::tor::Route::Onion => crate::tls::onion_config(),
                    crate::tor::Route::Exit => crate::tls::pinned_config(crate::tls::PROTON_PINS),
                }
                .map_err(Error::other_from)?,
                Arc::new(Quad9::through(tor)?),
                true,
            ),
        };
        let http = match way {
            Way::Direct => crate::tor::Connections::fixed(http_client_with(&base, tls, None, &dns)?),
            Way::Tor { tor, .. } => {
                let (proxy, at, quad9) = (tor.proxy_for_proton(), base.clone(), dns.clone());
                crate::tor::Connections::following(tor, move || {
                    http_client_with(&at, tls.clone(), Some(&proxy), &quad9)
                })?
            }
        };
        let tor = match way {
            Way::Direct => None,
            Way::Tor { tor, .. } => Some(tor.clone()),
        };
        let transport = Transport { http, dns, base, through_tor, tor };
        self.transport.set(transport).map_err(|_| Error::other("the way to Proton is chosen already"))?;
        log::info!(
            "the way to Proton: {}",
            match way {
                Way::Direct => "direct",
                Way::Tor { route: crate::tor::Route::Onion, .. } => "Tor, to Proton's onion site",
                Way::Tor { route: crate::tor::Route::Exit, .. } => "Tor, out of an exit",
            }
        );
        Ok(())
    }

    fn transport(&self) -> Result<&Transport> {
        self.transport.get().ok_or_else(|| Error::Network("no way to Proton is chosen yet".to_string()))
    }

    /// Whether everything goes through Tor.
    #[must_use]
    pub fn through_tor(&self) -> bool {
        self.transport.get().is_some_and(|transport| transport.through_tor)
    }

    pub fn dns(&self) -> Result<Arc<Quad9>> {
        Ok(self.transport()?.dns.clone())
    }

    pub fn on_session_change(&self, listener: SessionListener) {
        *self.listener.lock() = Some(listener);
    }

    /// For a process borrowing the app's session. A refresh rotates the tokens on the server, which
    /// would sign the app out, so an expired token simply ends the borrower's work.
    pub fn forbid_refresh(&self) {
        self.refresh_forbidden.store(true, Ordering::Relaxed);
    }

    /// For a check that must change nothing of the account, whatever is clicked: every request but a read is refused.
    pub fn forbid_changes(&self) {
        self.changes_forbidden.store(true, Ordering::Relaxed);
    }

    pub fn changes_forbidden(&self) -> bool {
        self.changes_forbidden.load(Ordering::Relaxed)
    }

    /// Keeps a record of every request from now on, for a check to summarize.
    pub fn record_requests(&self) -> Arc<RequestLog> {
        self.requests.get_or_init(|| Arc::new(RequestLog::new())).clone()
    }

    pub fn session(&self) -> Option<Session> {
        self.session.lock().clone()
    }

    fn tell(&self, change: SessionChange<'_>) {
        if let Some(listener) = self.listener.lock().as_ref() {
            listener(change);
        }
    }

    pub fn set_session(&self, session: Option<Session>) {
        if session.is_none() {
            self.kept.lock().clear();
        }
        (*self.session.lock()).clone_from(&session);
        match session {
            Some(session) => self.tell(SessionChange::Renewed(&session)),
            None => self.tell(SessionChange::Ended(Ending::Chosen)),
        }
    }

    pub fn end_session(&self, ending: Ending) {
        self.kept.lock().clear();
        *self.session.lock() = None;
        self.tell(SessionChange::Ended(ending));
    }

    /// While a person signs out the server revokes the session under any request still running.
    /// That refusal is what they asked for, so it must not read as Proton ending the session.
    pub fn set_leaving(&self, leaving: bool) {
        self.leaving.store(leaving, Ordering::Relaxed);
    }

    /// Lets go of the session in this process only. It stays valid on the server and in the store.
    pub fn drop_session(&self) {
        self.kept.lock().clear();
        *self.session.lock() = None;
    }

    /// Carries a challenge a person already completed on Proton's page into the next login.
    pub fn set_verification(&self, challenge: &HumanVerification) {
        let captcha = challenge.methods.iter().any(|m| m == "captcha");
        let method = if captcha { None } else { challenge.methods.first().cloned() };
        *self.verification.lock() = Some(Verification {
            token: challenge.token.clone(),
            method: method.unwrap_or_else(|| "captcha".to_string()),
        });
    }

    pub fn clear_verification(&self) {
        *self.verification.lock() = None;
    }

    fn headers_for(&self, no_auth: bool) -> reqwest::header::HeaderMap {
        let mut headers = HeaderMap::new();
        let mut set = |name: &'static str, value: &str| {
            if let Ok(value) = value.parse() {
                headers.insert(name, value);
            }
        };
        set("x-pm-appversion", APP_VERSION);
        if let Some(verification) = self.verification.lock().as_ref() {
            set("x-pm-human-verification-token", &verification.token);
            set("x-pm-human-verification-token-type", &verification.method);
        }
        if !no_auth && let Some(session) = self.session.lock().as_ref() {
            set("x-pm-uid", &session.uid);
            set("authorization", &format!("Bearer {}", session.access_token));
        }
        headers
    }

    /// One attempt, retried once after a rate limit or an expired access token.
    async fn send_with(
        &self,
        method: Method,
        path: &str,
        accept: &str,
        no_auth: bool,
        attach: &(dyn Fn(reqwest::RequestBuilder) -> reqwest::RequestBuilder + Send + Sync),
    ) -> Result<Response> {
        let refused = self.changes_forbidden() && method != Method::GET;
        if let Some(requests) = self.requests.get() {
            requests.record(method.as_str(), path, refused);
        }
        if refused {
            log::info!("{method} {}: refused, a check changes nothing", clean_path(path));
            return Err(Error::RefusedInCheck);
        }
        let transport = self.transport()?;
        let url = format!("{}{path}", transport.base);
        let mut retried = false;
        loop {
            let token = self.session.lock().as_ref().map(|s| s.access_token.clone());
            let request = transport
                .http
                .client()
                .request(method.clone(), &url)
                .headers(self.headers_for(no_auth))
                .header("accept", accept);
            let started = Instant::now();
            let response = attach(request).send().await.inspect_err(|_| {
                log::warn!("{method} {}: no answer", clean_path(path));
            })?;
            let status = response.status().as_u16();
            let took = started.elapsed();
            log::debug!("{method} {} -> {status} in {} ms", clean_path(path), took.as_millis());
            heard_back(transport, took);
            if retried {
                return Ok(response);
            }
            if status == 429 || status == 503 {
                let header = response.headers().get("retry-after").and_then(|v| v.to_str().ok());
                let delay = retry_delay(header);
                if delay <= MAX_RETRY_SECONDS {
                    log::info!("{status} from Proton, trying again in {delay} s");
                    tokio::time::sleep(Duration::from_secs_f64(delay)).await;
                    retried = true;
                    continue;
                }
            }
            // A 401 from the refresh itself is an answer. Refreshing again would wait for its own lock.
            let refreshable = path != REFRESH_PATH && !no_auth && token.is_some();
            if status == 401 && refreshable && !self.refresh_forbidden.load(Ordering::Relaxed) {
                Box::pin(self.refresh(token.as_deref())).await?;
                retried = true;
                continue;
            }
            return Ok(response);
        }
    }

    async fn call(&self, method: Method, path: &str, options: Options) -> Result<Value> {
        if method != Method::GET && !NOT_SETTINGS.iter().any(|mail| path.starts_with(mail)) {
            self.kept.lock().clear();
        }
        let body = options.body;
        let attach = move |request: reqwest::RequestBuilder| match &body {
            Some(value) => request.json(value),
            None => request,
        };
        let response = self.send_with(method, path, API_ACCEPT, options.no_auth, &attach).await?;
        self.finish(response, options.batch).await
    }

    async fn finish(&self, response: Response, batch: bool) -> Result<Value> {
        let status = response.status().as_u16();
        let bytes = response.bytes().await?;
        let body: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        check(status, &body, batch).inspect_err(|error| {
            let code =
                body.get("Code").and_then(Value::as_i64).map_or_else(|| "none".to_string(), |code| code.to_string());
            log::warn!("Proton answered {status}: {}, code {code}", error.kind());
        })?;
        Ok(body)
    }

    fn parse<T: DeserializeOwned>(body: Value) -> Result<T> {
        serde_json::from_value(body).map_err(|e| Error::other(format!("unexpected reply: {e}")))
    }

    fn kept_answer(&self, path: &str) -> Option<Kept> {
        let mut kept = self.kept.lock();
        let answer = kept.get_mut(path)?;
        if answer.at.elapsed() > KEPT_ANSWER_OLDEST {
            kept.remove(path);
            return None;
        }
        let body = answer.body.clone();
        if answer.at.elapsed() > KEPT_ANSWER_FRESH {
            answer.at = Instant::now();
            drop(kept);
            self.renew_quietly(path);
        }
        Some(body)
    }

    /// Nobody waits on this, so anything but a clean answer just leaves the kept one in place.
    fn renew_quietly(&self, path: &str) {
        let Ok(transport) = self.transport() else { return };
        let request = transport
            .http
            .client()
            .get(format!("{}{path}", transport.base))
            .headers(self.headers_for(false))
            .header("accept", API_ACCEPT);
        let (kept, path) = (self.kept.clone(), path.to_string());
        tokio::spawn(async move {
            let Ok(response) = request.send().await else { return };
            if !response.status().is_success() {
                return;
            }
            let Ok(body) = response.json::<Value>().await else { return };
            // Emptied meanwhile means a change or a sign-out: this answer may be from before it.
            let mut kept = kept.lock();
            if let Some(answer) = kept.get_mut(&path) {
                answer.body = Kept::Answer(body);
            }
        });
    }

    pub async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let keep = KEPT_PATHS.iter().any(|kept| path.starts_with(kept));
        if keep && let Some(kept) = self.kept_answer(path) {
            return Self::parse(kept.answer()?);
        }
        let answer = self.call(Method::GET, path, Options::default()).await;
        if keep && let Some(body) = Kept::of(&answer) {
            self.kept.lock().insert(path.to_string(), KeptAnswer { body, at: Instant::now() });
        }
        Self::parse(answer?)
    }

    pub async fn post<T: DeserializeOwned>(&self, path: &str, body: Value) -> Result<T> {
        let options = Options { body: Some(body), ..Default::default() };
        Self::parse(self.call(Method::POST, path, options).await?)
    }

    pub async fn post_unauthenticated<T: DeserializeOwned>(&self, path: &str, body: Value) -> Result<T> {
        let options = Options { body: Some(body), no_auth: true, ..Default::default() };
        Self::parse(self.call(Method::POST, path, options).await?)
    }

    pub async fn put<T: DeserializeOwned>(&self, path: &str, body: Value) -> Result<T> {
        let options = Options { body: Some(body), ..Default::default() };
        Self::parse(self.call(Method::PUT, path, options).await?)
    }

    pub async fn act(&self, method: Method, path: &str) -> Result<()> {
        self.call(method, path, Options::default()).await.map(drop)
    }

    pub async fn act_with(&self, method: Method, path: &str, body: Value) -> Result<()> {
        let options = Options { body: Some(body), ..Default::default() };
        self.call(method, path, options).await.map(drop)
    }

    /// A batch endpoint: Code 1001 with per-item results, any of which may have failed.
    pub async fn batch(&self, method: Method, path: &str, body: Value, what: &str) -> Result<()> {
        let options = Options { body: Some(body), batch: true, ..Default::default() };
        let reply = self.call(method, path, options).await?;
        first_batch_failure(&reply).map_or(Ok(()), |reason| Err(Error::other(format!("{what}: {reason}"))))
    }

    /// A raw body with the headers that came with it, such as an attachment or an image.
    pub async fn fetch(&self, path: &str) -> Result<(Vec<u8>, reqwest::header::HeaderMap)> {
        let attach = |request: reqwest::RequestBuilder| request;
        let response = self.send_with(Method::GET, path, "*/*", false, &attach).await?;
        let status = response.status();
        if !status.is_success() {
            return Err(Error::Api { status: status.as_u16(), code: 0, message: format!("HTTP {}", status.as_u16()) });
        }
        let headers = response.headers().clone();
        Ok((response.bytes().await?.to_vec(), headers))
    }

    pub async fn upload<T: DeserializeOwned>(&self, path: &str, form: &(dyn Fn() -> Form + Send + Sync)) -> Result<T> {
        let attach = |request: reqwest::RequestBuilder| request.multipart(form());
        let response = self.send_with(Method::POST, path, API_ACCEPT, false, &attach).await?;
        Self::parse(self.finish(response, false).await?)
    }

    /// An upload to a batch endpoint, which answers Code 1001 with a result for each item.
    pub async fn upload_batch<T: DeserializeOwned>(
        &self,
        path: &str,
        form: &(dyn Fn() -> Form + Send + Sync),
    ) -> Result<T> {
        let attach = |request: reqwest::RequestBuilder| request.multipart(form());
        let response = self.send_with(Method::POST, path, API_ACCEPT, false, &attach).await?;
        Self::parse(self.finish(response, true).await?)
    }

    /// Concurrent 401s share one round trip: whoever waited finds the token already new.
    async fn refresh(&self, used_token: Option<&str>) -> Result<()> {
        let _guard = self.refreshing.lock().await;
        let Some(session) = self.session() else {
            return Err(Error::SessionEnded);
        };
        if used_token != Some(session.access_token.as_str()) {
            return Ok(());
        }
        log::info!("the access token expired, renewing it");
        match self.renew(&session).await {
            Err(error) if refuses_session(&error) => self.confirm_refusal(&session).await,
            other => other.inspect_err(|error| log::warn!("no new token yet: {}", error.kind())),
        }
    }

    /// Only a second refusal ends the session. Losing the network, or Proton having a bad moment,
    /// leaves it as it is: the next request tries again.
    async fn confirm_refusal(&self, session: &Session) -> Result<()> {
        if self.leaving.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        log::warn!("Proton refused the token, asking once more");
        tokio::time::sleep(REFUSAL_RECHECK).await;
        // Locked or signed out meanwhile: this is no longer the session in question.
        if self.session().as_ref() != Some(session) || self.leaving.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        match self.renew(session).await {
            Err(error) if refuses_session(&error) => {
                log::warn!("Proton refused the token twice, the session is over");
                self.end_session(Ending::Refused);
                Err(Error::SessionEnded)
            }
            other => other,
        }
    }

    async fn renew(&self, session: &Session) -> Result<()> {
        let options = Options {
            body: Some(json!({
                "UID": session.uid,
                "RefreshToken": session.refresh_token,
                "AccessToken": session.access_token,
                "ResponseType": "token",
                "GrantType": "refresh_token",
                "RedirectURI": "https://protonmail.ch",
                "State": random_hex(),
            })),
            ..Default::default()
        };
        let tokens: Tokens = Self::parse(self.call(Method::POST, REFRESH_PATH, options).await?)?;
        // Locked meanwhile: nothing of the account may come back into this process.
        if self.session().is_none() {
            return Ok(());
        }
        log::info!("the access token was renewed");
        self.set_session(Some(Session {
            uid: session.uid.clone(),
            access_token: tokens.access_token.clone(),
            refresh_token: tokens.refresh_token.clone(),
            password_mode: session.password_mode,
        }));
        Ok(())
    }
}

#[derive(Deserialize, Zeroize, ZeroizeOnDrop)]
#[serde(rename_all = "PascalCase")]
struct Tokens {
    access_token: String,
    refresh_token: String,
}

#[cfg(test)]
mod tests;
