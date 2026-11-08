//! What Proton guards with the account password: the ways back into an account, its second factor
//! and Sentinel. The person types the password, for this one action. It goes through SRP, so it never
//! leaves this computer, and it is not kept. A refusal is reported and never tried again by itself:
//! failures can lock an account.

use super::{Deserialize, encoded, Error, json, Method, on_worker, Result, Service, Value, Zeroizing};
use crate::crypto::AuthInfo;

const REAUTH_INFO: &str = "/core/v4/auth/info";
const UNLOCK_CHANGES: &str = "/core/v4/users/password";
const LOCK_AGAIN: &str = "/core/v4/users/lock";
const TOTP: &str = "/core/v4/settings/2fa/totp";
const TOTP_SECRET: &str = "/core/v4/settings/2fa/totp/secret";

/// What the person typed. It lives as long as the one call it is passed to.
pub struct Confirmation {
    pub password: Zeroizing<String>,
    /// The code of an authenticator app, on an account that has one.
    pub code: Option<Zeroizing<String>>,
}

/// A secret to put into an authenticator app, before it has been confirmed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TotpSetup {
    pub secret: String,
    /// The same as a QR code holds it.
    pub uri: String,
}

#[derive(Deserialize)]
struct ReauthInfo {
    #[serde(rename = "Version")]
    version: u8,
    #[serde(rename = "Modulus")]
    modulus: String,
    #[serde(rename = "ServerEphemeral")]
    server_ephemeral: String,
    #[serde(rename = "Salt")]
    salt: String,
    #[serde(rename = "SRPSession")]
    srp_session: String,
}

fn proof_fields(proof: &str, ephemeral: &str, session: &str, code: Option<&str>, more: Value) -> Value {
    let mut body = json!({"ClientProof": proof, "ClientEphemeral": ephemeral, "SRPSession": session});
    if let Some(code) = code {
        crate::mapping::put(&mut body, "TwoFactorCode", Value::from(code));
    }
    if let (Some(body), Value::Object(more)) = (body.as_object_mut(), more) {
        body.extend(more);
    }
    body
}

impl Service {
    /// One call carrying the proof of the password. Whatever answers must prove in turn that it knows
    /// the password too, or it is not Proton.
    async fn with_proof(&self, path: &str, more: Value, confirmation: &Confirmation) -> Result<Value> {
        let body = json!({"Intent": "Proton", "ReauthScope": "password"});
        let info: ReauthInfo = self.client.post(REAUTH_INFO, body).await?;
        if !matches!(info.version, 3 | 4) {
            return Err(Error::other(format!("Unsupported auth version {}", info.version)));
        }
        let user: Value = self.client.get("/core/v4/users").await?;
        let name = user.pointer("/User/Name").and_then(Value::as_str).unwrap_or_default().to_string();
        let crypto = self.crypto.clone();
        let secret = confirmation.password.clone();
        let details = AuthInfo { version: info.version, modulus: info.modulus, server_ephemeral: info.server_ephemeral, salt: info.salt };
        let proof = on_worker(move || crypto.login_proof(&name, &secret, &details)).await?;
        let code = confirmation.code.as_ref().map(|code| code.as_str());
        let body = proof_fields(&proof.proof, &proof.ephemeral, &info.srp_session, code, more);
        let reply: Value = self.client.put(path, body).await?;
        if reply.get("ServerProof").and_then(Value::as_str) != Some(proof.expected_server_proof.as_str()) {
            return Err(Error::other("Server proof mismatch: refusing to trust this server"));
        }
        Ok(reply)
    }

    /// Opens the guarded settings for the call that follows, and closes them again whatever it did.
    async fn guarded(&self, confirmation: &Confirmation, path: &str, body: Value) -> Result<()> {
        self.with_proof(UNLOCK_CHANGES, json!({}), confirmation).await?;
        let done = self.client.act_with(Method::PUT, path, body).await;
        let _ = self.client.act(Method::PUT, LOCK_AGAIN).await;
        done
    }

    pub async fn set_recovery_email(&self, email: &str, confirmation: &Confirmation) -> Result<()> {
        self.guarded(confirmation, "/core/v4/settings/email", json!({"Email": email})).await
    }

    pub async fn set_recovery_phone(&self, phone: &str, confirmation: &Confirmation) -> Result<()> {
        self.guarded(confirmation, "/core/v4/settings/phone", json!({"Phone": phone})).await
    }

    pub async fn set_email_reset(&self, on: bool, confirmation: &Confirmation) -> Result<()> {
        self.guarded(confirmation, "/core/v4/settings/email/reset", json!({"Reset": u8::from(on)})).await
    }

    pub async fn set_phone_reset(&self, on: bool, confirmation: &Confirmation) -> Result<()> {
        self.guarded(confirmation, "/core/v4/settings/phone/reset", json!({"Reset": u8::from(on)})).await
    }

    pub async fn set_log_mode(&self, mode: u8, confirmation: &Confirmation) -> Result<()> {
        self.guarded(confirmation, "/core/v4/settings/logauth", json!({"LogAuth": mode})).await
    }

    pub async fn set_sentinel(&self, on: bool, confirmation: &Confirmation) -> Result<()> {
        self.with_proof(UNLOCK_CHANGES, json!({}), confirmation).await?;
        let method = if on { Method::POST } else { Method::DELETE };
        let done = self.client.act(method, "/core/v4/settings/highsecurity").await;
        let _ = self.client.act(Method::PUT, LOCK_AGAIN).await;
        done
    }

    pub async fn disable_totp(&self, confirmation: &Confirmation) -> Result<()> {
        self.with_proof(TOTP, json!({}), confirmation).await.map(drop)
    }

    /// The first half of turning the second factor on. Nothing changes until `confirm_totp`.
    pub async fn start_totp(&self, confirmation: &Confirmation, account: &str) -> Result<TotpSetup> {
        self.with_proof(UNLOCK_CHANGES, json!({}), confirmation).await?;
        let reply: Value = self.client.get(TOTP_SECRET).await?;
        let secret = reply.get("Secret").and_then(Value::as_str).unwrap_or_default().to_string();
        if secret.is_empty() {
            return Err(Error::other("Proton sent no secret"));
        }
        let uri = totp_uri(account, &secret);
        Ok(TotpSetup { secret, uri })
    }

    /// Answers with the recovery codes, which are shown this once.
    pub async fn confirm_totp(&self, code: &str) -> Result<Vec<String>> {
        let reply: Value = self.client.post(TOTP, json!({"TOTPConfirmation": code})).await?;
        let _ = self.client.act(Method::PUT, LOCK_AGAIN).await;
        let codes = reply.get("TwoFactorRecoveryCodes").and_then(Value::as_array).map_or(&[][..], Vec::as_slice);
        Ok(codes.iter().filter_map(Value::as_str).map(str::to_string).collect())
    }
}

/// As `getTOTPData` in Proton's client has it.
fn totp_uri(account: &str, secret: &str) -> String {
    format!("otpauth://totp/{}?secret={secret}&issuer=Proton&algorithm=SHA1&digits=6&period=30", encoded(account))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_proof_goes_with_the_code_when_there_is_one() {
        let body = proof_fields("p", "e", "s", Some("123456"), json!({}));
        assert_eq!(body, json!({"ClientProof": "p", "ClientEphemeral": "e", "SRPSession": "s", "TwoFactorCode": "123456"}));
    }

    #[test]
    fn a_proof_without_a_code_says_nothing_of_one() {
        assert!(proof_fields("p", "e", "s", None, json!({})).get("TwoFactorCode").is_none());
    }

    #[test]
    fn what_a_call_adds_cannot_be_lost() {
        assert_eq!(proof_fields("p", "e", "s", None, json!({"Email": "a@b.c"})).get("Email"), Some(&json!("a@b.c")));
    }

    #[test]
    fn an_account_name_cannot_break_out_of_the_uri() {
        let uri = totp_uri("a b?c@d.e", "SECRET");
        assert!(uri.starts_with("otpauth://totp/a"));
        assert_eq!(uri.matches('?').count(), 1);
    }
}
