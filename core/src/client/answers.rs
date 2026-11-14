//! What Proton's answers mean, whatever their HTTP status says: its own code, the human verification it asks for,
//! the items of a batch it did not act on, and when to ask again.

use super::{CODE_BATCH, CODE_HUMAN_VERIFICATION, CODE_OK, DEFAULT_RETRY_SECONDS};
use crate::error::{Error, HumanVerification, Result};
use serde_json::Value;

pub(super) fn retry_delay(header: Option<&str>) -> f64 {
    header
        .and_then(|value| value.parse::<f64>().ok())
        .filter(|seconds| seconds.is_finite() && *seconds >= 0.0)
        .unwrap_or(DEFAULT_RETRY_SECONDS)
}

pub(super) fn verification_in(body: &Value) -> Option<HumanVerification> {
    let details = body.get("Details")?;
    let token = details.get("HumanVerificationToken")?.as_str().filter(|t| !t.is_empty())?;
    let methods = details.get("HumanVerificationMethods").and_then(Value::as_array);
    Some(HumanVerification {
        token: token.to_string(),
        methods: methods.map_or_default(|list| list.iter().filter_map(Value::as_str).map(str::to_string).collect()),
        web_url: details.get("WebUrl").and_then(Value::as_str).unwrap_or_default().to_string(),
    })
}

/// Only these answers to a refresh rule the session invalid, as in `refreshHandlers.ts` of Proton's
/// web client. Anything else, a 409 or 429 or 5xx among them, means trying again later.
pub(super) const fn refuses_session(error: &Error) -> bool {
    matches!(error, Error::Api { status: 400 | 401 | 422, .. })
}

/// Proton's verdict on a parsed reply, whatever the HTTP status says.
pub(super) fn check(status: u16, body: &Value, batch: bool) -> Result<()> {
    let code = body.get("Code").and_then(Value::as_i64);
    if code == Some(CODE_HUMAN_VERIFICATION)
        && let Some(challenge) = verification_in(body)
    {
        return Err(Error::Verification(challenge));
    }
    let accepted = matches!(code, None | Some(CODE_OK)) || (batch && code == Some(CODE_BATCH));
    if (200..300).contains(&status) && accepted {
        return Ok(());
    }
    let message = body.get("Error").and_then(Value::as_str);
    Err(Error::Api {
        status,
        code: code.unwrap_or(0),
        message: message.map_or_else(|| format!("HTTP {status}"), str::to_string),
    })
}

/// Items of a batch reply that Proton did not act on.
pub fn first_batch_failure(body: &Value) -> Option<String> {
    let failed = body.get("Responses")?.as_array()?.iter().find_map(|item| {
        let response = item.get("Response")?;
        (response.get("Code")?.as_i64() != Some(CODE_OK)).then_some(response)
    })?;
    let code = failed.get("Code").and_then(Value::as_i64).unwrap_or(0);
    Some(failed.get("Error").and_then(Value::as_str).map_or_else(|| format!("code {code}"), str::to_string))
}
