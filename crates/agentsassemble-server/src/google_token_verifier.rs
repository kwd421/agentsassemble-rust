use crate::google_accounts::GoogleAccountError;
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header, jwk::JwkSet};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{sync::Arc, time::Duration};
use subtle::ConstantTimeEq;
use tokio::{sync::Mutex, time::Instant};

const GOOGLE_KEYS_URL: &str = "https://www.googleapis.com/oauth2/v3/certs";
const MAX_KEYS_BYTES: usize = 64 * 1024;
const UNKNOWN_KEY_REFRESH_INTERVAL: Duration = Duration::from_mins(1);

pub(crate) struct GoogleTokenVerifier {
    client: reqwest::Client,
    client_id: String,
    keys: Mutex<Option<TrustedKeys>>,
}

struct TrustedKeys {
    keys: Arc<JwkSet>,
    loaded_at: Instant,
    policy: http_cache_semantics::CachePolicy,
}

#[derive(Deserialize)]
struct GoogleClaims {
    sub: String,
    nonce: String,
    iat: u64,
    azp: Option<String>,
}

impl GoogleTokenVerifier {
    pub(crate) fn new(client_id: String) -> Result<Self, GoogleAccountError> {
        let client = reqwest::Client::builder()
            .https_only(true)
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(8))
            .user_agent("AgentsAssemble/1.0")
            .build()
            .map_err(|_| GoogleAccountError::unavailable())?;
        Ok(Self {
            client,
            client_id,
            keys: Mutex::new(None),
        })
    }

    pub(crate) async fn verify(
        &self,
        credential: &str,
        nonce: &str,
    ) -> Result<[u8; 32], GoogleAccountError> {
        if credential.is_empty() || credential.len() > 16_384 {
            return Err(GoogleAccountError::invalid());
        }
        let header = decode_header(credential).map_err(|_| GoogleAccountError::invalid())?;
        if header.alg != Algorithm::RS256 {
            return Err(GoogleAccountError::invalid());
        }
        let kid = header
            .kid
            .filter(|kid| !kid.is_empty() && kid.len() <= 128)
            .ok_or_else(GoogleAccountError::invalid)?;
        let keys = self.trusted_keys(&kid).await?;
        verify_google_claims(credential, nonce, &self.client_id, &kid, &keys)
    }

    async fn trusted_keys(&self, kid: &str) -> Result<Arc<JwkSet>, GoogleAccountError> {
        let mut cached = self.keys.lock().await;
        let now = Instant::now();
        if let Some(current) = cached.as_ref()
            && now.duration_since(current.loaded_at) < Duration::from_hours(1)
            && matches!(
                current
                    .policy
                    .before_request(&google_key_request()?, std::time::SystemTime::now()),
                http_cache_semantics::BeforeRequest::Fresh(_)
            )
            && (current.keys.find(kid).is_some()
                || now.duration_since(current.loaded_at) < UNKNOWN_KEY_REFRESH_INTERVAL)
        {
            return Ok(current.keys.clone());
        }
        let mut response = self
            .client
            .get(GOOGLE_KEYS_URL)
            .send()
            .await
            .map_err(|_| GoogleAccountError::unavailable())?;
        if !response.status().is_success()
            || response
                .content_length()
                .is_some_and(|length| length > MAX_KEYS_BYTES as u64)
        {
            return Err(GoogleAccountError::unavailable());
        }
        let policy = key_cache_policy(response.status(), response.headers())?;
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| GoogleAccountError::unavailable())?
        {
            if bytes.len().saturating_add(chunk.len()) > MAX_KEYS_BYTES {
                return Err(GoogleAccountError::unavailable());
            }
            bytes.extend_from_slice(&chunk);
        }
        let keys: JwkSet =
            serde_json::from_slice(&bytes).map_err(|_| GoogleAccountError::unavailable())?;
        if keys.keys.is_empty() {
            return Err(GoogleAccountError::unavailable());
        }
        let keys = Arc::new(keys);
        let loaded_at = Instant::now();
        *cached = policy.is_storable().then(|| TrustedKeys {
            keys: keys.clone(),
            loaded_at,
            policy,
        });
        Ok(keys)
    }
}

fn verify_google_claims(
    credential: &str,
    nonce: &str,
    client_id: &str,
    kid: &str,
    keys: &JwkSet,
) -> Result<[u8; 32], GoogleAccountError> {
    let jwk = keys.find(kid).ok_or_else(GoogleAccountError::invalid)?;
    let key = DecodingKey::from_jwk(jwk).map_err(|_| GoogleAccountError::invalid())?;
    let mut validation = Validation::new(Algorithm::RS256);
    validation.set_audience(&[client_id]);
    validation.set_issuer(&["https://accounts.google.com", "accounts.google.com"]);
    validation.set_required_spec_claims(&["exp", "iss", "aud", "sub"]);
    validation.leeway = 0;
    validation.validate_nbf = true;
    let claims = decode::<GoogleClaims>(credential, &key, &validation)
        .map_err(|_| GoogleAccountError::invalid())?
        .claims;
    let now = u64::try_from(chrono::Utc::now().timestamp())
        .map_err(|_| GoogleAccountError::unavailable())?;
    if claims.sub.is_empty()
        || claims.sub.len() > 512
        || claims.iat > now
        || claims
            .azp
            .as_deref()
            .is_some_and(|presenter| presenter != client_id)
        || !bool::from(claims.nonce.as_bytes().ct_eq(nonce.as_bytes()))
    {
        return Err(GoogleAccountError::invalid());
    }
    let mut fingerprint = Sha256::new();
    fingerprint.update(b"google\0");
    fingerprint.update(claims.sub.as_bytes());
    Ok(fingerprint.finalize().into())
}

fn google_key_request() -> Result<axum::http::Request<()>, GoogleAccountError> {
    axum::http::Request::get(GOOGLE_KEYS_URL)
        .body(())
        .map_err(|_| GoogleAccountError::unavailable())
}

fn key_cache_policy(
    status: axum::http::StatusCode,
    headers: &axum::http::HeaderMap,
) -> Result<http_cache_semantics::CachePolicy, GoogleAccountError> {
    let request = google_key_request()?;
    let mut metadata = axum::http::Response::builder()
        .status(status)
        .body(())
        .map_err(|_| GoogleAccountError::unavailable())?;
    *metadata.headers_mut() = headers.clone();
    Ok(http_cache_semantics::CachePolicy::new_options(
        &request,
        &metadata,
        std::time::SystemTime::now(),
        http_cache_semantics::CacheOptions {
            shared: true,
            cache_heuristic: 0.0,
            immutable_min_time_to_live: Duration::ZERO,
            ignore_cargo_cult: false,
        },
    ))
}

#[cfg(test)]
impl GoogleTokenVerifier {
    pub(crate) async fn install_fixture_keys(&self, keys: JwkSet) {
        let now = Instant::now();
        *self.keys.lock().await = Some(TrustedKeys {
            keys: Arc::new(keys),
            loaded_at: now,
            policy: key_cache_policy(
                axum::http::StatusCode::OK,
                &axum::http::HeaderMap::from_iter([(
                    axum::http::header::CACHE_CONTROL,
                    axum::http::HeaderValue::from_static("public, max-age=3600"),
                )]),
            )
            .unwrap_or_else(|error| panic!("fixture cache: {error}")),
        });
    }
}
