use crate::google_token_verifier::GoogleTokenVerifier;
use agentsassemble_persistence::AccountIdentity;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use ring::rand::{SecureRandom, SystemRandom};
use serde::Serialize;
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::{sync::Mutex, time::Instant};

const CHALLENGE_TTL: Duration = Duration::from_mins(5);
const MAX_CHALLENGES: usize = 512;
const MAX_UNBOUND_CHALLENGES: usize = 64;

#[derive(Clone)]
pub struct GoogleAccountService {
    client_id: String,
    verifier: Option<Arc<GoogleTokenVerifier>>,
    challenges: Arc<Mutex<HashMap<String, Challenge>>>,
}

struct Challenge {
    identity: AccountIdentity,
    expires_at: Instant,
}

#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct GoogleAccountError {
    pub code: &'static str,
    pub message: &'static str,
}

impl GoogleAccountError {
    pub(crate) const fn invalid() -> Self {
        Self {
            code: "google_credential_invalid",
            message: "Google could not verify this login.",
        }
    }
    pub(crate) const fn unavailable() -> Self {
        Self {
            code: "google_verification_unavailable",
            message: "Google login verification is temporarily unavailable.",
        }
    }
    const fn challenge() -> Self {
        Self {
            code: "google_login_challenge_invalid",
            message: "Google login challenge expired or was already used.",
        }
    }
}

#[derive(Serialize)]
pub struct GoogleAccountConfiguration<'a> {
    pub enabled: bool,
    pub client_id: &'a str,
    pub unavailable_reason: &'static str,
}

#[derive(Serialize)]
pub struct GoogleAccountChallenge {
    pub status: &'static str,
    pub client_id: String,
    pub nonce: String,
}

impl GoogleAccountService {
    /// Creates the configured verifier without contacting Google. Empty configuration disables login.
    ///
    /// # Errors
    /// Rejects malformed configuration or an unavailable HTTPS client.
    pub fn new(client_id: &str) -> Result<Self, GoogleAccountError> {
        let client_id = client_id.trim();
        if client_id.len() > 512 || !client_id.bytes().all(|byte| byte.is_ascii_graphic()) {
            return Err(GoogleAccountError {
                code: "google_configuration_invalid",
                message: "Google web client ID is invalid.",
            });
        }
        let verifier = if client_id.is_empty() {
            None
        } else {
            Some(Arc::new(GoogleTokenVerifier::new(client_id.into())?))
        };
        Ok(Self {
            client_id: client_id.into(),
            verifier,
            challenges: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    /// Reads the original Google web-client configuration once at runtime startup.
    ///
    /// # Errors
    /// Rejects non-Unicode or malformed configuration and HTTPS client failure.
    pub fn from_environment() -> Result<Self, GoogleAccountError> {
        match std::env::var("AGENTSASSEMBLE_GOOGLE_WEB_CLIENT_ID") {
            Ok(client_id) => Self::new(&client_id),
            Err(std::env::VarError::NotPresent) => Ok(Self::default()),
            Err(std::env::VarError::NotUnicode(_)) => Err(GoogleAccountError {
                code: "google_configuration_invalid",
                message: "Google web client ID is invalid.",
            }),
        }
    }

    #[must_use]
    pub fn configuration(&self) -> GoogleAccountConfiguration<'_> {
        GoogleAccountConfiguration {
            enabled: self.verifier.is_some(),
            client_id: &self.client_id,
            unavailable_reason: if self.verifier.is_some() {
                ""
            } else {
                "google_client_id_missing"
            },
        }
    }

    /// Issues a bounded one-use challenge for the exact server/device identity.
    ///
    /// # Errors
    /// Rejects disabled login, exhausted capacity or unavailable randomness.
    pub async fn start(
        &self,
        identity: AccountIdentity,
    ) -> Result<GoogleAccountChallenge, GoogleAccountError> {
        self.require_verifier()?;
        let mut bytes = [0; 32];
        SystemRandom::new()
            .fill(&mut bytes)
            .map_err(|_| GoogleAccountError::unavailable())?;
        let nonce = URL_SAFE_NO_PAD.encode(bytes);
        let now = Instant::now();
        let mut challenges = self.challenges.lock().await;
        challenges.retain(|_, challenge| {
            challenge.expires_at > now && !same_challenge_owner(&identity, &challenge.identity)
        });
        let unbound_full = identity.user().is_none()
            && challenges
                .values()
                .filter(|challenge| challenge.identity.user().is_none())
                .count()
                >= MAX_UNBOUND_CHALLENGES;
        if challenges.len() >= MAX_CHALLENGES || unbound_full {
            return Err(GoogleAccountError {
                code: "google_login_capacity_exceeded",
                message: "Google login capacity is temporarily exhausted.",
            });
        }
        challenges.insert(
            nonce.clone(),
            Challenge {
                identity,
                expires_at: now + CHALLENGE_TTL,
            },
        );
        Ok(GoogleAccountChallenge {
            status: "ready",
            client_id: self.client_id.clone(),
            nonce,
        })
    }

    /// Verifies Google proof and atomically consumes its matching login challenge.
    ///
    /// # Errors
    /// Rejects unknown, changed, expired or replayed challenges and invalid/unavailable Google proof.
    pub async fn verify(
        &self,
        identity: &AccountIdentity,
        credential: &str,
        nonce: &str,
    ) -> Result<[u8; 32], GoogleAccountError> {
        let verifier = self.require_verifier()?;
        self.validate_challenge(identity, nonce, false).await?;
        let fingerprint = verifier.verify(credential, nonce).await?;
        self.validate_challenge(identity, nonce, true).await?;
        Ok(fingerprint)
    }

    async fn validate_challenge(
        &self,
        identity: &AccountIdentity,
        nonce: &str,
        consume: bool,
    ) -> Result<(), GoogleAccountError> {
        let mut challenges = self.challenges.lock().await;
        let now = Instant::now();
        challenges.retain(|_, challenge| challenge.expires_at > now);
        if !challenges
            .get(nonce)
            .is_some_and(|challenge| challenge.identity.same_login_subject(identity))
        {
            return Err(GoogleAccountError::challenge());
        }
        if consume {
            challenges.remove(nonce);
        }
        Ok(())
    }

    fn require_verifier(&self) -> Result<&GoogleTokenVerifier, GoogleAccountError> {
        self.verifier.as_deref().ok_or(GoogleAccountError {
            code: "google_login_not_configured",
            message: "Google login is not configured on this server.",
        })
    }
}

impl Default for GoogleAccountService {
    fn default() -> Self {
        Self {
            client_id: String::new(),
            verifier: None,
            challenges: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

fn same_challenge_owner(left: &AccountIdentity, right: &AccountIdentity) -> bool {
    match (left.user(), right.user()) {
        (Some(left), Some(right)) => left.user_id == right.user_id,
        _ => left.same_login_subject(right),
    }
}

#[cfg(test)]
#[path = "google_account_service_tests.rs"]
pub(crate) mod tests;
