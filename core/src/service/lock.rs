//! The app password: sealing the saved session, and what stands in the way of guessing it.

use super::{
    AuthStep, Error, Instant, Latency, LockState, Result, Service, Value, WRONG_PASSWORD_PAUSE, Zeroizing, now_seconds,
    on_worker, vault,
};

impl Service {
    pub fn app_lock_enabled(&self) -> bool {
        self.store.load().is_some_and(|saved| serde_json::from_str::<vault::Envelope>(&saved).is_ok())
    }

    pub async fn set_app_password(&self, password: &str) -> Result<()> {
        let session = self.client.session().ok_or_else(|| Error::other("Sign in first"))?;
        let salt = vault::new_salt();
        let phrase = Zeroizing::new(password.to_string());
        let key = on_worker(move || vault::derive_key(&phrase, &salt)).await?;
        *self.lock.lock() = LockState { key: Some(key), salt: salt.to_vec() };
        self.persist(&session);
        Ok(())
    }

    pub async fn remove_app_password(&self, password: &str) -> Result<()> {
        self.try_password(password).await?;
        *self.lock.lock() = LockState::default();
        let session = self.client.session().ok_or_else(|| Error::other("Sign in first"))?;
        self.persist(&session);
        Ok(())
    }

    pub(super) async fn check_password(
        &self,
        password: &str,
    ) -> Result<(vault::VaultKey, Zeroizing<Vec<u8>>, Vec<u8>)> {
        let saved = self.store.load().ok_or(Error::WrongPassword)?;
        let envelope: vault::Envelope = serde_json::from_str(&saved).map_err(|_| Error::WrongPassword)?;
        let salt = vault::salt_of(&envelope)?;
        let (phrase, for_key) = (Zeroizing::new(password.to_string()), salt.clone());
        let key = on_worker(move || vault::derive_key(&phrase, &for_key)).await?;
        let plain = vault::open(&key, &envelope)?;
        Ok((key, plain, salt))
    }

    pub(super) fn with_attempts<T>(&self, use_them: impl FnOnce(&mut vault::Attempts) -> T) -> T {
        let mut attempts = self.attempts.lock();
        use_them(attempts.get_or_insert_with(|| {
            let saved = self.store.load_attempts();
            saved.and_then(|value| serde_json::from_str(&value).ok()).unwrap_or_default()
        }))
    }

    pub(super) fn reset_attempts(&self) {
        *self.attempts.lock() = Some(vault::Attempts::default());
        self.store.clear_attempts();
    }

    /// Seconds until a password may be tried again. Touch ID is not held back: the system guards it.
    pub fn unlock_wait(&self) -> u64 {
        self.with_attempts(|attempts| attempts.wait(now_seconds()))
    }

    /// Argon2 already costs a guesser; the pause and the freezes stop one at the keyboard.
    pub(super) async fn try_password(&self, password: &str) -> Result<(vault::VaultKey, Zeroizing<Vec<u8>>, Vec<u8>)> {
        let _one_at_a_time = self.password_gate.lock().await;
        let seconds = self.unlock_wait();
        if seconds > 0 {
            return Err(Error::TooManyAttempts { seconds });
        }
        let result = self.check_password(password).await;
        match &result {
            Ok(_) => self.reset_attempts(),
            Err(Error::WrongPassword) => {
                self.with_attempts(|attempts| {
                    attempts.failed(now_seconds());
                    if let Ok(value) = serde_json::to_string(attempts) {
                        self.store.save_attempts(&value);
                    }
                });
                log::warn!("a wrong app password");
                tokio::time::sleep(WRONG_PASSWORD_PAUSE).await;
            }
            Err(_) => {}
        }
        result
    }

    pub async fn unlock_app(&self, password: &str) -> Result<AuthStep> {
        let (key, plain, salt) = self.try_password(password).await?;
        *self.lock.lock() = LockState { key: Some(key), salt };
        self.resume(&plain).await
    }

    /// Touch ID hands the key back from the keychain, after the system confirmed the person.
    pub async fn unlock_with_key(&self, key: vault::VaultKey) -> Result<AuthStep> {
        let saved = self.store.load().ok_or(Error::WrongPassword)?;
        let envelope: vault::Envelope = serde_json::from_str(&saved).map_err(|_| Error::WrongPassword)?;
        let plain = vault::open(&key, &envelope)?;
        *self.lock.lock() = LockState { key: Some(key), salt: vault::salt_of(&envelope)? };
        self.reset_attempts();
        self.resume(&plain).await
    }

    pub fn lock_key(&self) -> Option<vault::VaultKey> {
        self.lock.lock().key.clone()
    }

    /// Clears the process, and leaves the sealed session where it is. Returns false if no password
    /// is set: then nothing could bring the session back, so nothing is touched.
    pub fn lock_now(&self) -> bool {
        if self.lock.lock().key.is_none() {
            return false;
        }
        // The secret goes first: a token refresh landing now must not save the session unsealed.
        log::info!("locking");
        self.wipe_memory();
        *self.lock.lock() = LockState::default();
        self.client.drop_session();
        true
    }

    /// For a forgotten app password. The session stays open on Proton's side until revoked there.
    pub fn forget_locked_session(&self) {
        log::info!("giving up the saved session");
        self.client.set_session(None);
    }

    /// Whether Proton answers at all. No session, no account: only what the connection is doing.
    pub async fn ping(&self) -> Result<()> {
        self.client.get::<Value>("/tests/ping").await.map(drop)
    }

    /// Asked twice and the faster counts: the first may have to open a connection, and this is
    /// meant to show the road and not the handshake.
    pub async fn latency(&self) -> Latency {
        async fn fastest<F: Future<Output = Result<()>>>(probe: impl Fn() -> F) -> Option<u32> {
            let mut best = None;
            for _ in 0..2 {
                let started = Instant::now();
                probe().await.ok()?;
                let taken = u32::try_from(started.elapsed().as_millis()).unwrap_or(u32::MAX);
                best = Some(best.map_or(taken, |known: u32| known.min(taken)));
            }
            best
        }
        let dns = self.client.dns().ok();
        let quad9_ping = || async {
            match &dns {
                Some(dns) => dns.ping().await,
                None => Err(Error::Network("no way chosen".to_string())),
            }
        };
        let (proton, quad9) = futures::join!(fastest(|| self.ping()), fastest(quad9_ping));
        Latency { proton, quad9 }
    }

    pub fn forbid_token_refresh(&self) {
        self.client.forbid_refresh();
    }

    pub fn is_signed_in(&self) -> bool {
        self.client.session().is_some() && self.key_secret.lock().is_some()
    }
}
