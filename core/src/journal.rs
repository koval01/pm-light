//! The app's own log.
//!
//! It follows everything the app does, stays encrypted on disk, and is gone after a day. Each record is sealed on
//! its own, so the key is the only way in and a torn tail costs one record. Code writes through the `log` macros.
//! Nothing that names a person or holds mail may be passed to them: use numbers, statuses, error kinds and
//! `clean_path`.

use chacha20poly1305::{
    KeyInit, XChaCha20Poly1305, XNonce,
    aead::{Aead, Payload},
};
use log::{Level, LevelFilter, Log, Metadata};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{
        OnceLock,
        mpsc::{Receiver, RecvTimeoutError, SyncSender, sync_channel},
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use zeroize::Zeroizing;

pub const KEY_BYTES: usize = 32;
pub type JournalKey = Zeroizing<[u8; KEY_BYTES]>;

const RETENTION_HOURS: u64 = 24;
/// Past this an hour keeps only warnings and errors: a loop must not fill the disk.
const HOUR_LIMIT: u64 = 8 * 1024 * 1024;
const QUEUE: usize = 4096;
const PRUNE_EVERY: Duration = Duration::from_secs(600);
const FLUSH_WAIT: Duration = Duration::from_millis(300);
const TEXT_LIMIT: usize = 600;
const NONCE_BYTES: usize = 24;
const LENGTH_BYTES: usize = 4;
const CONTEXT: &[u8] = b"proton-mail-light journal 1";
const EXTENSION: &str = "journal";
const OWN_CODE: &str = "proton_";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    /// Milliseconds since the epoch.
    pub time: u64,
    pub level: String,
    pub area: String,
    pub text: String,
}

enum Command {
    Write(Record),
    Flush(SyncSender<()>),
    Clear(SyncSender<()>),
}

struct Logger {
    queue: SyncSender<Command>,
}

struct Shared {
    dir: PathBuf,
    cipher: OnceLock<XChaCha20Poly1305>,
    queue: SyncSender<Command>,
}

static JOURNAL: OnceLock<Shared> = OnceLock::new();

#[must_use]
pub fn new_key() -> JournalKey {
    let mut key = Zeroizing::new([0; KEY_BYTES]);
    rand::thread_rng().fill_bytes(key.as_mut());
    key
}

fn now_millis() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| {
        u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
    })
}

const fn hour_of(millis: u64) -> u64 {
    millis / 3_600_000
}

/// Cut at a character boundary: the text may hold anything.
fn clip(text: &str) -> String {
    text.chars().take(TEXT_LIMIT).collect()
}

/// A built program writes the journal to the terminal too when started with `PML_JOURNAL_ECHO` set, for measuring
/// the build that ships. The records are the same as the journal's, with nothing of the person's in them.
static ECHO: std::sync::LazyLock<bool> = std::sync::LazyLock::new(|| std::env::var_os("PML_JOURNAL_ECHO").is_some());

/// Message and address ids are the account's own business: only the shape of a path is kept.
#[must_use]
pub fn clean_path(path: &str) -> String {
    let path = path.split_once('?').map_or(path, |(path, _)| path);
    // Proton's endpoints are short lowercase words, such as `organizations`; ids, names and
    // addresses are anything else.
    let is_word = |part: &str| part.len() <= 24 && part.bytes().all(|b| b.is_ascii_lowercase() || b == b'-');
    let looks_like_id =
        |part: &str| !is_word(part) && (part.len() > 12 || part.contains(['=', '@', '+', '%', '.']));
    let parts: Vec<&str> = path.split('/').map(|part| if looks_like_id(part) { "{id}" } else { part }).collect();
    parts.join("/")
}

/// Tor's own crates, arti's: they log through `tracing`, which hands its events on to `log`. Their messages pass
/// through arti's `safelog`, which scrubs addresses and names before anything is written.
fn is_tors(target: &str) -> bool {
    target.starts_with("tor_") || target.starts_with("arti")
}

/// Everything of the app's own; of Tor's, what it says at `info` and above, which is its life without the noise.
fn wanted(target: &str, level: log::Level) -> bool {
    target.starts_with(OWN_CODE) || (is_tors(target) && level <= Level::Info)
}

fn area_of(target: &str) -> &str {
    if is_tors(target) {
        return target.split("::").next().unwrap_or(target);
    }
    let own = target.strip_prefix(OWN_CODE).unwrap_or(target);
    match own.split_once("::") {
        Some((_, inner)) => inner,
        None if own == "mail_light" => "app",
        None => own,
    }
}

impl Log for Logger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        wanted(metadata.target(), metadata.level())
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let text = clip(&record.args().to_string());
        if cfg!(debug_assertions) || *ECHO {
            eprintln!("{} {}: {text}", record.level(), record.target());
        }
        let entry = Record {
            time: now_millis(),
            level: record.level().as_str().to_lowercase(),
            area: area_of(record.target()).to_string(),
            text,
        };
        // A full queue loses a record. The app must never wait for its own log.
        let _ = self.queue.try_send(Command::Write(entry));
    }

    fn flush(&self) {
        let (done, wait) = sync_channel(1);
        if self.queue.try_send(Command::Flush(done)).is_ok() {
            let _ = wait.recv_timeout(FLUSH_WAIT);
        }
    }
}

fn seal(cipher: &XChaCha20Poly1305, record: &Record) -> Option<Vec<u8>> {
    let nonce: [u8; NONCE_BYTES] = rand::random();
    let plain = Zeroizing::new(serde_json::to_vec(record).ok()?);
    let sealed =
        cipher.encrypt(&XNonce::from(nonce), Payload { msg: &plain, aad: CONTEXT }).ok()?;
    let length = u32::try_from(NONCE_BYTES + sealed.len()).ok()?;
    let mut frame = Vec::with_capacity(LENGTH_BYTES + NONCE_BYTES + sealed.len());
    frame.extend_from_slice(&length.to_le_bytes());
    frame.extend_from_slice(&nonce);
    frame.extend_from_slice(&sealed);
    Some(frame)
}

/// What can be opened of a file: a damaged record is skipped, a torn end stops the reading.
fn open_frames(cipher: &XChaCha20Poly1305, mut bytes: &[u8], out: &mut Vec<Record>) {
    while let Some((length, rest)) = bytes.split_first_chunk::<LENGTH_BYTES>() {
        let length = u32::from_le_bytes(*length) as usize;
        let Some((frame, after)) = rest.split_at_checked(length) else { return };
        bytes = after;
        let Some((nonce, sealed)) = frame.split_at_checked(NONCE_BYTES) else { continue };
        let Ok(nonce) = XNonce::try_from(nonce) else { continue };
        let opened = cipher.decrypt(&nonce, Payload { msg: sealed, aad: CONTEXT });
        if let Some(record) = opened.ok().and_then(|plain| serde_json::from_slice(&plain).ok()) {
            out.push(record);
        }
    }
}

fn hour_files(dir: &Path) -> Vec<(u64, PathBuf)> {
    let Ok(entries) = fs::read_dir(dir) else { return Vec::new() };
    let mut files: Vec<(u64, PathBuf)> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension()?.to_str()? != EXTENSION {
                return None;
            }
            Some((path.file_stem()?.to_str()?.parse().ok()?, path))
        })
        .collect();
    files.sort_by_key(|(hour, _)| *hour);
    files
}

/// A file from the future means the clock moved, and such a file would otherwise outlive its day.
fn prune(dir: &Path, current_hour: u64) {
    for (hour, path) in hour_files(dir) {
        if hour + RETENTION_HOURS < current_hour || hour > current_hour + 1 {
            let _ = fs::remove_file(path);
        }
    }
}

fn read_from(dir: &Path, cipher: &XChaCha20Poly1305, limit: usize) -> Vec<Record> {
    let mut newest_first = Vec::new();
    for (_, path) in hour_files(dir).into_iter().rev() {
        let mut records = Vec::new();
        open_frames(cipher, &fs::read(path).unwrap_or_default(), &mut records);
        newest_first.extend(records.into_iter().rev());
        if newest_first.len() >= limit {
            break;
        }
    }
    newest_first.truncate(limit);
    newest_first
}

fn open_private(path: &Path) -> Option<File> {
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(path).ok()
}

struct Writer {
    dir: PathBuf,
    cipher: XChaCha20Poly1305,
    current: Option<(u64, File, u64)>,
}

impl Writer {
    fn file_for(&mut self, hour: u64) -> Option<(&mut File, &mut u64)> {
        if self.current.as_ref().is_none_or(|(open, ..)| *open != hour) {
            let path = self.dir.join(format!("{hour}.{EXTENSION}"));
            let file = open_private(&path)?;
            let size = file.metadata().map_or(0, |meta| meta.len());
            self.current = Some((hour, file, size));
            prune(&self.dir, hour);
        }
        self.current.as_mut().map(|(_, file, size)| (file, size))
    }

    fn write(&mut self, record: &Record) {
        let urgent = record.level == "error" || record.level == "warn";
        let Some(frame) = seal(&self.cipher, record) else { return };
        let Some((file, size)) = self.file_for(hour_of(record.time)) else { return };
        if *size < HOUR_LIMIT || urgent {
            *size += frame.len() as u64;
            let _ = file.write_all(&frame);
        }
    }

    fn clear(&mut self) {
        self.current = None;
        for (_, path) in hour_files(&self.dir) {
            let _ = fs::remove_file(path);
        }
    }

    fn run(mut self, commands: &Receiver<Command>) {
        loop {
            match commands.recv_timeout(PRUNE_EVERY) {
                Ok(Command::Write(record)) => self.write(&record),
                Ok(Command::Flush(done)) => drop(done.try_send(())),
                Ok(Command::Clear(done)) => {
                    self.clear();
                    let _ = done.try_send(());
                }
                Err(RecvTimeoutError::Timeout) => prune(&self.dir, hour_of(now_millis())),
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
    }
}

fn prepare_folder(dir: &Path) -> bool {
    let made = fs::create_dir_all(dir).is_ok();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(dir, fs::Permissions::from_mode(0o700));
    }
    made
}

/// Records wait in the queue while `key` runs, since asking the keychain can take a person's click.
/// Without a key nothing is written: a plain log would be worse than none.
pub fn start(dir: PathBuf, key: impl FnOnce() -> Option<JournalKey> + Send + 'static) {
    let (queue, commands) = sync_channel(QUEUE);
    if log::set_boxed_logger(Box::new(Logger { queue: queue.clone() })).is_err() {
        return;
    }
    log::set_max_level(LevelFilter::Debug);
    let shared = JOURNAL.get_or_init(|| Shared { dir: dir.clone(), cipher: OnceLock::new(), queue });
    let started = thread::Builder::new().name("journal".into()).spawn(move || {
        // First, and without the key: a machine that was off for a week owes its old records now.
        prune(&dir, hour_of(now_millis()));
        let cipher = key().and_then(|key| XChaCha20Poly1305::new_from_slice(key.as_slice()).ok());
        let Some(cipher) = cipher.filter(|_| prepare_folder(&dir)) else {
            commands.iter().for_each(drop);
            return;
        };
        let _ = shared.cipher.set(cipher.clone());
        Writer { dir, cipher, current: None }.run(&commands);
    });
    if let Err(error) = started {
        eprintln!("no journal: {error}");
    }
}

/// The newest records first, for the app's own viewer.
pub fn read(limit: usize) -> Vec<Record> {
    let Some(journal) = JOURNAL.get() else { return Vec::new() };
    log::logger().flush();
    journal.cipher.get().map_or_else(Vec::new, |cipher| read_from(&journal.dir, cipher, limit))
}

pub fn clear() {
    let Some(journal) = JOURNAL.get() else { return };
    let (done, wait) = sync_channel(1);
    if journal.queue.try_send(Command::Clear(done)).is_ok() {
        let _ = wait.recv_timeout(FLUSH_WAIT);
    }
}

/// A note from the page, which has no way to the `log` macros of its own.
pub fn note(level: &str, text: &str) {
    let level = match level {
        "error" => Level::Error,
        "warn" => Level::Warn,
        _ => Level::Info,
    };
    log::log!(target: "proton_page", level, "{text}");
}

#[cfg(test)]
mod tests;
