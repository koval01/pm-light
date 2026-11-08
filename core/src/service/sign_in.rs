//! Signing in and out, and reopening a saved session.

use super::{Address, AddressInput, AuthInfo, AuthStep, Deserialize, Ending, Engine, Error, json, KeySaltRow, Link, LockedKey, Login, mail_store, Method, number, on_worker, Result, SavedSession, Service, Session, Stage, StageListener, STANDARD, text, TWO_FACTOR_TOTP, Value, vault, Zeroizing};

impl Service {
    pub async fn sign_in(&self, username: &str, password: &str) -> Result<AuthStep> {
        log::info!("signing in");
        {
            let mut login = self.login.lock();
            login.username = username.trim().to_string();
            *login.password = password.to_string();
        }
        self.attempt_login().await
    }

    pub(super) async fn attempt_login(&self) -> Result<AuthStep> {
        let (username, password) = {
            let login = self.login.lock();
            (login.username.clone(), login.password.clone())
        };
        match self.authenticate(&username, &password).await {
            Ok(true) => Ok(AuthStep::Totp),
            Ok(false) => self.after_authentication().await,
            Err(Error::Verification(challenge)) => {
                self.client.clear_verification();
                self.login.lock().challenge = Some(challenge.clone());
                Ok(AuthStep::Verify { challenge })
            }
            Err(error) => Err(error),
        }
    }

    /// Returns whether a second factor is still needed.
    pub(super) async fn authenticate(&self, username: &str, password: &str) -> Result<bool> {
        #[derive(Deserialize)]
        #[serde(rename_all = "PascalCase")]
        struct Info {
            version: u8,
            modulus: String,
            server_ephemeral: String,
            salt: String,
            #[serde(rename = "SRPSession")]
            srp_session: String,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "PascalCase")]
        struct Auth {
            #[serde(rename = "UID")]
            uid: String,
            access_token: String,
            refresh_token: String,
            server_proof: String,
            password_mode: u8,
            #[serde(rename = "2FA", default)]
            two_factor: Value,
        }

        let info: Info = self
            .client
            .post_unauthenticated("/auth/v4/info", json!({"Username": username, "Intent": "Proton"}))
            .await?;
        if !matches!(info.version, 3 | 4) {
            return Err(Error::other(format!(
                "Unsupported auth version {}: change the account password once in Proton to upgrade it",
                info.version
            )));
        }
        let crypto = self.crypto.clone();
        let (user, secret) = (username.to_string(), Zeroizing::new(password.to_string()));
        let details = AuthInfo {
            version: info.version,
            modulus: info.modulus,
            server_ephemeral: info.server_ephemeral,
            salt: info.salt,
        };
        let proof = on_worker(move || crypto.login_proof(&user, &secret, &details)).await?;
        let auth: Auth = self
            .client
            .post_unauthenticated(
                "/auth/v4",
                json!({
                    "Username": username,
                    "ClientEphemeral": proof.ephemeral,
                    "ClientProof": proof.proof,
                    "SRPSession": info.srp_session,
                }),
            )
            .await?;
        if auth.server_proof != proof.expected_server_proof {
            return Err(Error::other("Server proof mismatch: refusing to trust this server"));
        }
        self.client.clear_verification();
        self.client.set_session(Some(Session {
            uid: auth.uid,
            access_token: auth.access_token,
            refresh_token: auth.refresh_token,
            password_mode: auth.password_mode,
        }));
        let enabled = auth.two_factor.get("Enabled").and_then(Value::as_u64).unwrap_or(0);
        if enabled != 0 && enabled & TWO_FACTOR_TOTP == 0 {
            let _ = self.sign_out().await; // no half-open session listed on the account
            return Err(Error::other(
                "This account's second factor is a security key (FIDO2), which this client cannot do. \
                 Add an authenticator app in Proton's settings.",
            ));
        }
        Ok(enabled != 0)
    }

    /// `solved` is what Proton's embedded page handed back: the proof of this very check.
    pub async fn continue_after_verification(&self, solved: Option<(String, String)>) -> Result<AuthStep> {
        let Some(mut challenge) = self.login.lock().challenge.take() else {
            return Ok(AuthStep::Credentials);
        };
        if let Some((token, kind)) = solved {
            challenge.token = token;
            challenge.methods = vec![kind];
        }
        self.client.set_verification(&challenge);
        self.attempt_login().await.inspect_err(|_| self.cancel_login())
    }

    /// Forgets a sign-in in progress. A wrong password cannot be fixed from a later screen.
    pub fn cancel_login(&self) {
        self.client.clear_verification();
        *self.login.lock() = Login::default();
    }

    pub async fn submit_totp(&self, code: &str) -> Result<AuthStep> {
        log::info!("second factor sent");
        let digits: String = code.chars().filter(|c| !c.is_whitespace()).collect();
        self.client.act_with(Method::POST, "/auth/v4/2fa", json!({"TwoFactorCode": digits})).await?;
        self.after_authentication().await
    }

    pub(super) async fn after_authentication(&self) -> Result<AuthStep> {
        if self.client.session().is_some_and(|s| s.password_mode == 2) {
            return Ok(AuthStep::Mailbox);
        }
        let password = self.login.lock().password.clone();
        self.unlock(&password).await
    }

    pub async fn submit_mailbox_password(&self, password: &str) -> Result<AuthStep> {
        self.unlock(password).await
    }

    pub(super) async fn fetch_key_material(&self) -> Result<(Vec<LockedKey>, Vec<Value>)> {
        #[derive(Deserialize)]
        struct UserReply {
            #[serde(rename = "User")]
            user: UserKeys,
        }
        #[derive(Deserialize)]
        struct UserKeys {
            #[serde(rename = "Keys")]
            keys: Vec<LockedKey>,
        }
        let user = self.client.get::<UserReply>("/core/v4/users");
        let addresses = self.client.get::<Value>("/core/v4/addresses");
        let (user, addresses) = futures::try_join!(user, addresses)?;
        let rows = addresses.get("Addresses").and_then(Value::as_array).cloned().unwrap_or_default();
        Ok((user.user.keys, rows))
    }

    pub(super) async fn unlock(&self, password: &str) -> Result<AuthStep> {
        #[derive(Deserialize)]
        #[serde(rename_all = "PascalCase")]
        struct Salts {
            key_salts: Vec<KeySaltRow>,
        }
        let (material, salts) = futures::try_join!(
            self.fetch_key_material(),
            self.client.get::<Salts>("/core/v4/keys/salts")
        )?;
        let crypto = self.crypto.clone();
        let (keys, rows) = material;
        let (keys_for_secret, password) = (keys.clone(), Zeroizing::new(password.to_string()));
        let secret =
            on_worker(move || crypto.key_secret(&password, &keys_for_secret, &salts.key_salts)).await?;
        self.open(keys, rows, Zeroizing::new(secret)).await?;
        *self.login.lock() = Login::default();
        Ok(AuthStep::Ready)
    }

    pub(super) async fn open(&self, user_keys: Vec<LockedKey>, rows: Vec<Value>, secret: Zeroizing<Vec<u8>>) -> Result<()> {
        let mut rows = rows;
        rows.sort_by_key(|row| row.get("Order").and_then(Value::as_u64).unwrap_or(0));
        let inputs = rows
            .iter()
            .map(|row| {
                let keys = row.get("Keys").cloned().unwrap_or(Value::Array(vec![]));
                Ok(AddressInput {
                    id: text(row, "ID"),
                    keys: serde_json::from_value(keys).map_err(|e| Error::other(e.to_string()))?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let crypto = self.crypto.clone();
        let opening = secret.clone();
        on_worker(move || crypto.unlock(user_keys, inputs, &opening)).await?;
        *self.addresses.write() = rows
            .iter()
            .map(|row| Address {
                id: text(row, "ID"),
                email: text(row, "Email"),
                display_name: text(row, "DisplayName"),
                signature: text(row, "Signature"),
                can_send: number(row, "Status") == 1 && number(row, "Send") == 1,
            })
            .collect();
        let session = self.client.session().ok_or_else(|| Error::other("The session ended"))?;
        *self.key_secret.lock() = Some(secret);
        self.open_mail_store();
        self.persist(&session);
        Ok(())
    }

    pub fn on_stage(&self, listener: StageListener) {
        *self.stage_listener.lock() = Some(listener);
    }

    fn reach(&self, stage: Stage) {
        log::info!("start: {stage:?}");
        if let Some(listener) = self.stage_listener.lock().as_ref() {
            listener(stage);
        }
    }

    /// The start of the app: a saved session Proton cannot answer for yet is waited on, not lost.
    pub async fn reopen(&self) -> AuthStep {
        // The page came back after resting: nothing here ever closed.
        if self.key_secret.lock().is_some() {
            return AuthStep::Ready;
        }
        self.restore().await.unwrap_or_else(|error| AuthStep::Waiting {
            link: error.link().unwrap_or(Link::Unavailable),
        })
    }

    /// Reopens the last session. Only Proton's refusal forgets it: a machine that starts offline,
    /// or a bad minute on Proton's side, must find the account still there.
    pub async fn restore(&self) -> Result<AuthStep> {
        self.reach(Stage::Session);
        let store = self.store.clone();
        // On a worker: this call lasts for as long as the system's question stays unanswered.
        let Some(saved) = on_worker(move || Ok(store.load())).await?.map(Zeroizing::new) else {
            return Ok(AuthStep::Credentials);
        };
        if serde_json::from_str::<vault::Envelope>(&saved).is_ok() {
            return Ok(AuthStep::Locked);
        }
        self.resume(saved.as_bytes()).await
    }

    pub(super) async fn resume(&self, plain: &[u8]) -> Result<AuthStep> {
        let Ok(saved) = serde_json::from_slice::<SavedSession>(plain) else {
            self.store.clear();
            return Ok(AuthStep::Credentials);
        };
        let Ok(secret) = STANDARD.decode(saved.key_secret.as_bytes()).map(Zeroizing::new) else {
            self.store.clear();
            return Ok(AuthStep::Credentials);
        };
        log::info!("reopening the saved session");
        let store_key = saved.store_key.as_ref().and_then(|text| STANDARD.decode(text.as_bytes()).ok());
        *self.store_key.lock() = store_key
            .and_then(|bytes| <[u8; mail_store::KEY_BYTES]>::try_from(bytes).ok())
            .map(Zeroizing::new);
        self.client.set_session(Some(saved.session.clone()));
        self.reach(Stage::Connecting);
        let reopened = async {
            let (keys, rows) = self.fetch_key_material().await?;
            self.reach(Stage::Keys);
            self.open(keys, rows, secret).await
        };
        match reopened.await {
            Ok(()) => {
                log::info!("the session is open");
                Ok(AuthStep::Ready)
            }
            Err(Error::SessionEnded) => Ok(AuthStep::Credentials),
            // The saved secret no longer opens the keys, and no amount of waiting will change that.
            Err(Error::Crypto(_)) => {
                log::warn!("the saved secret no longer opens the keys");
                self.client.end_session(Ending::Refused);
                Ok(AuthStep::Credentials)
            }
            Err(error) => {
                log::warn!("the session waits: {}", error.kind());
                Err(error)
            }
        }
    }

    pub async fn sign_out(&self) -> Result<()> {
        log::info!("signing out");
        self.client.set_leaving(true);
        let result = self.client.act(Method::DELETE, "/auth/v4").await;
        self.client.set_session(None);
        self.client.set_leaving(false);
        result
    }
}
