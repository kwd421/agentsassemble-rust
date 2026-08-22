use std::{
    collections::HashMap,
    fmt::{self, Write},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use agentsassemble_protocol::TicketResponse;
use axum::http::HeaderMap;
use hmac::{Hmac, Mac};
use serde::Serialize;
use sha2::Sha256;
use subtle::ConstantTimeEq;
use thiserror::Error;
use uuid::Uuid;

const CHALLENGE_CONTEXT: &str = "agentsassemble-host-challenge-v1\0";
const REQUEST_CONTEXT: &str = "agentsassemble-host-ticket-request-v1\0";
const RESPONSE_CONTEXT: &str = "agentsassemble-host-ticket-response-v1\0";
const CHALLENGE_TTL: Duration = Duration::from_secs(30);
const MAX_PENDING_CHALLENGES: usize = 512;

#[derive(Clone)]
pub struct HostSecret {
    value: Arc<str>,
    pending_challenges: Arc<Mutex<HashMap<String, Instant>>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("host secret must contain at least 32 non-whitespace bytes")]
pub struct InvalidHostSecret;

pub(crate) struct AuthenticatedTicketRequest {
    pub(crate) challenge: String,
    pub(crate) meeting_id: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct HostChallengeResponse {
    challenge: String,
    host_challenge_proof: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct AuthenticatedTicketResponse {
    #[serde(flatten)]
    grant: TicketResponse,
    host_response_proof: String,
}

impl HostSecret {
    /// Validates a desktop runtime host credential.
    ///
    /// # Errors
    ///
    /// Returns `InvalidHostSecret` for short or whitespace-padded credentials.
    pub fn new(value: impl Into<String>) -> Result<Self, InvalidHostSecret> {
        let value = value.into();
        if value.len() < 32 || value.trim() != value {
            return Err(InvalidHostSecret);
        }
        Ok(Self {
            value: Arc::from(value),
            pending_challenges: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    pub(crate) fn challenge(&self) -> Option<HostChallengeResponse> {
        let challenge = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        let now = Instant::now();
        let mut pending = self.pending_challenges.lock().ok()?;
        pending.retain(|_, expires_at| *expires_at > now);
        if pending.len() >= MAX_PENDING_CHALLENGES
            && let Some(oldest) = pending
                .iter()
                .min_by_key(|(_, expires_at)| **expires_at)
                .map(|(challenge, _)| challenge.clone())
        {
            pending.remove(&oldest);
        }
        pending.insert(challenge.clone(), now + CHALLENGE_TTL);
        let host_challenge_proof = sign_fields(&self.value, CHALLENGE_CONTEXT, &[&challenge]);
        Some(HostChallengeResponse {
            challenge,
            host_challenge_proof,
        })
    }

    pub(crate) fn authenticate_ticket_request(
        &self,
        headers: &HeaderMap,
    ) -> Option<AuthenticatedTicketRequest> {
        let challenge = header(headers, "x-host-challenge")?;
        let meeting_id = header(headers, "x-host-meeting")?;
        let proof = header(headers, "x-host-proof")?;
        if challenge.len() != 64
            || !challenge.bytes().all(|byte| byte.is_ascii_hexdigit())
            || proof.len() != 64
        {
            return None;
        }
        let expected = sign_fields(&self.value, REQUEST_CONTEXT, &[&challenge, &meeting_id]);
        if !bool::from(expected.as_bytes().ct_eq(proof.as_bytes())) {
            return None;
        }
        let mut pending = self.pending_challenges.lock().ok()?;
        if !matches!(pending.remove(&challenge), Some(expires_at) if expires_at > Instant::now()) {
            return None;
        }
        Some(AuthenticatedTicketRequest {
            challenge,
            meeting_id,
        })
    }

    pub(crate) fn authenticated_ticket_response(
        &self,
        challenge: &str,
        grant: TicketResponse,
    ) -> AuthenticatedTicketResponse {
        let ttl = grant.ttl_seconds.to_string();
        let host_response_proof = sign_fields(
            &self.value,
            RESPONSE_CONTEXT,
            &[challenge, &grant.ticket, &ttl, &grant.server_proof_key],
        );
        AuthenticatedTicketResponse {
            grant,
            host_response_proof,
        }
    }
}

impl fmt::Debug for HostSecret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("HostSecret([REDACTED])")
    }
}

fn header(headers: &HeaderMap, name: &'static str) -> Option<String> {
    headers.get(name)?.to_str().ok().map(str::to_owned)
}

fn sign_fields(key: &str, context: &str, fields: &[&str]) -> String {
    let mut signer = Hmac::<Sha256>::new_from_slice(key.as_bytes())
        .unwrap_or_else(|_| unreachable!("HMAC accepts keys of every length"));
    signer.update(context.as_bytes());
    for field in fields {
        signer.update(field.as_bytes());
        signer.update(&[0]);
    }
    signer
        .finalize()
        .into_bytes()
        .iter()
        .fold(String::with_capacity(64), |mut encoded, byte| {
            let _ = write!(encoded, "{byte:02x}");
            encoded
        })
}

#[cfg(test)]
mod tests {
    use agentsassemble_protocol::TicketResponse;
    use axum::http::{HeaderMap, HeaderValue};

    use super::HostSecret;

    #[test]
    fn request_and_response_match_the_browser_contract_and_bind_every_field() {
        let secret = HostSecret::new("host-token-0000000000000000000000")
            .unwrap_or_else(|error| panic!("create host secret: {error}"));
        let challenge = "c".repeat(64);
        assert_eq!(
            super::sign_fields(
                &secret.value,
                super::REQUEST_CONTEXT,
                &[&challenge, "general"],
            ),
            "d09a57843280b3ff939f5aac64629f11a93b2ef4f536972bcc962974ef78152c"
        );
        let issued = secret
            .challenge()
            .unwrap_or_else(|| panic!("issue host challenge"));
        assert_eq!(issued.challenge.len(), 64);
        let proof = super::sign_fields(
            &secret.value,
            super::REQUEST_CONTEXT,
            &[&issued.challenge, "general"],
        );
        assert_eq!(
            proof,
            super::sign_fields(
                &secret.value,
                super::REQUEST_CONTEXT,
                &[&issued.challenge, "general"],
            )
        );
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-host-challenge",
            HeaderValue::from_str(&issued.challenge)
                .unwrap_or_else(|error| panic!("challenge header: {error}")),
        );
        headers.insert("x-host-meeting", HeaderValue::from_static("general"));
        headers.insert(
            "x-host-proof",
            HeaderValue::from_str(&proof).unwrap_or_else(|error| panic!("proof header: {error}")),
        );
        let request = secret
            .authenticate_ticket_request(&headers)
            .unwrap_or_else(|| panic!("authenticate host request"));
        assert_eq!(request.meeting_id, "general");
        assert!(secret.authenticate_ticket_request(&headers).is_none());
        headers.insert("x-host-meeting", HeaderValue::from_static("changed"));
        assert!(secret.authenticate_ticket_request(&headers).is_none());
        let response = secret.authenticated_ticket_response(
            &challenge,
            TicketResponse {
                ticket: "a".repeat(64),
                ttl_seconds: 30,
                server_proof_key: "b".repeat(64),
            },
        );
        assert_eq!(
            response.host_response_proof,
            "8fe3585f8818361e6eaa5d2a3c91d6da17686881d667e1b2b1a292954dd0d486"
        );
        assert_eq!(
            super::sign_fields(&secret.value, super::CHALLENGE_CONTEXT, &[&challenge],),
            "663e4232010a2500a2ee27f392029d1f8bdb8c03ab75e34bfdf39cff63d77144"
        );
    }
}
