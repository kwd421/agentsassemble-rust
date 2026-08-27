use std::{collections::BTreeMap, sync::Arc};

use agentsassemble_persistence::PersistentHostIdentity;
use axum::http::Uri;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::Utc;
use ring::{
    rand::{SecureRandom, SystemRandom},
    signature::{Ed25519KeyPair, KeyPair},
};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use thiserror::Error;

const REGISTRATION_CONTEXT: &str = "AA-HOST-REGISTER-1";
const REGISTRATION_NONCE_BYTES: usize = 18;
const SERVER_CHALLENGE_CONTEXT: &str = "AA-SERVER-CHALLENGE-1";

#[derive(Debug, Error)]
pub enum HostIdentityError {
    #[error("persistent Ed25519 host private key is invalid")]
    InvalidPrivateKey,
    #[error("host identity JSON projection failed")]
    Json(#[source] serde_json::Error),
    #[error("host registration entropy source failed")]
    Entropy,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub(crate) enum ServerChallengeError {
    #[error("challenge must be 22-128 base64url characters")]
    InvalidChallenge,
    #[error("server origin must be HTTPS, or loopback HTTP for local use")]
    InvalidOrigin,
}

#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct HostPublicJwk {
    crv: &'static str,
    ext: bool,
    key_ops: [&'static str; 1],
    kty: &'static str,
    x: String,
}

#[derive(Serialize)]
pub struct HostRegistrationProof {
    owner_person_id: String,
    issued_at: i64,
    nonce: String,
    signature: String,
}

#[derive(Serialize)]
pub struct HostRegistrationEnvelope {
    server_id: String,
    host_public_key_jwk: HostPublicJwk,
    host_key_fingerprint: String,
    host_registration_proof: HostRegistrationProof,
}

#[derive(Serialize)]
pub(crate) struct ServerInfoEnvelope {
    server_id: String,
    host_public_key_jwk: HostPublicJwk,
    host_key_fingerprint: String,
    protocol_version: u32,
    status: &'static str,
    central_directory: CentralDirectoryStatus,
}

#[derive(Serialize)]
struct CentralDirectoryStatus {
    enabled: bool,
}

#[derive(Serialize)]
pub(crate) struct ServerChallengeEnvelope {
    server_id: String,
    origin: String,
    host_public_key_jwk: HostPublicJwk,
    host_key_fingerprint: String,
    protocol_version: u32,
    challenge: String,
    issued_at: i64,
    signature: String,
}

#[derive(Clone)]
pub struct CentralHostIdentity {
    server_id: Arc<str>,
    key_pair: Arc<Ed25519KeyPair>,
    public_jwk: HostPublicJwk,
    fingerprint: Arc<str>,
}

impl CentralHostIdentity {
    /// Builds the public signing projection from the file-owned private key.
    ///
    /// # Errors
    ///
    /// Rejects invalid Ed25519 material or an unserializable public projection.
    pub fn from_persistent(identity: &PersistentHostIdentity) -> Result<Self, HostIdentityError> {
        let key_pair = Ed25519KeyPair::from_pkcs8(identity.private_key_pkcs8())
            .map_err(|_| HostIdentityError::InvalidPrivateKey)?;
        let public_jwk = HostPublicJwk {
            crv: "Ed25519",
            ext: true,
            key_ops: ["verify"],
            kty: "OKP",
            x: URL_SAFE_NO_PAD.encode(key_pair.public_key().as_ref()),
        };
        let canonical = canonical_jwk(&public_jwk)?;
        let fingerprint = URL_SAFE_NO_PAD.encode(Sha256::digest(canonical));
        Ok(Self {
            server_id: identity.server_id().into(),
            key_pair: Arc::new(key_pair),
            public_jwk,
            fingerprint: fingerprint.into(),
        })
    }

    pub(crate) fn server_id(&self) -> &str {
        &self.server_id
    }

    pub(crate) fn public_key_x(&self) -> &str {
        &self.public_jwk.x
    }

    pub(crate) fn fingerprint(&self) -> &str {
        &self.fingerprint
    }

    pub(crate) fn server_info(&self) -> ServerInfoEnvelope {
        ServerInfoEnvelope {
            server_id: self.server_id.to_string(),
            host_public_key_jwk: self.public_jwk.clone(),
            host_key_fingerprint: self.fingerprint.to_string(),
            protocol_version: agentsassemble_protocol::PROTOCOL_VERSION,
            status: "ready",
            central_directory: CentralDirectoryStatus { enabled: false },
        }
    }

    pub(crate) fn challenge_envelope(
        &self,
        challenge: &str,
        origin: &str,
    ) -> Result<ServerChallengeEnvelope, ServerChallengeError> {
        let challenge = challenge.trim();
        if !(22..=128).contains(&challenge.len())
            || !challenge
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Err(ServerChallengeError::InvalidChallenge);
        }
        let origin = normalize_server_identity_origin(origin)?;
        let issued_at = Utc::now().timestamp();
        let transcript = format!(
            "{SERVER_CHALLENGE_CONTEXT}\n{}\n{origin}\n{challenge}\n{issued_at}",
            self.server_id
        );
        let signature = URL_SAFE_NO_PAD.encode(self.key_pair.sign(transcript.as_bytes()).as_ref());
        Ok(ServerChallengeEnvelope {
            server_id: self.server_id.to_string(),
            origin,
            host_public_key_jwk: self.public_jwk.clone(),
            host_key_fingerprint: self.fingerprint.to_string(),
            protocol_version: agentsassemble_protocol::PROTOCOL_VERSION,
            challenge: challenge.to_owned(),
            issued_at,
            signature,
        })
    }

    /// Creates one fresh central-directory registration proof.
    ///
    /// # Errors
    ///
    /// Returns an entropy error when the OS random source is unavailable.
    pub fn registration_envelope(
        &self,
        owner_person_id: &str,
    ) -> Result<HostRegistrationEnvelope, HostIdentityError> {
        let issued_at = Utc::now().timestamp();
        let mut nonce_bytes = [0_u8; REGISTRATION_NONCE_BYTES];
        SystemRandom::new()
            .fill(&mut nonce_bytes)
            .map_err(|_| HostIdentityError::Entropy)?;
        let nonce = URL_SAFE_NO_PAD.encode(nonce_bytes);
        let transcript = format!(
            "{REGISTRATION_CONTEXT}\n{}\n{owner_person_id}\n{issued_at}\n{nonce}",
            self.server_id
        );
        let signature = URL_SAFE_NO_PAD.encode(self.key_pair.sign(transcript.as_bytes()).as_ref());
        Ok(HostRegistrationEnvelope {
            server_id: self.server_id.to_string(),
            host_public_key_jwk: self.public_jwk.clone(),
            host_key_fingerprint: self.fingerprint.to_string(),
            host_registration_proof: HostRegistrationProof {
                owner_person_id: owner_person_id.to_owned(),
                issued_at,
                nonce,
                signature,
            },
        })
    }
}

fn normalize_server_identity_origin(value: &str) -> Result<String, ServerChallengeError> {
    let clean = value.trim().trim_end_matches('/');
    let uri = clean
        .parse::<Uri>()
        .map_err(|_| ServerChallengeError::InvalidOrigin)?;
    let scheme = uri
        .scheme_str()
        .ok_or(ServerChallengeError::InvalidOrigin)?;
    let authority = uri.authority().ok_or(ServerChallengeError::InvalidOrigin)?;
    if authority.as_str().contains('@') || uri.query().is_some() || uri.path() != "/" {
        return Err(ServerChallengeError::InvalidOrigin);
    }
    let host = authority
        .host()
        .trim_matches(['[', ']'])
        .to_ascii_lowercase();
    let loopback = matches!(host.as_str(), "localhost" | "127.0.0.1" | "::1");
    if host.is_empty() || (scheme != "https" && !(scheme == "http" && loopback)) {
        return Err(ServerChallengeError::InvalidOrigin);
    }
    let normalized_host = if host.contains(':') {
        format!("[{host}]")
    } else {
        host
    };
    Ok(match authority.port_u16() {
        Some(port) => format!("{scheme}://{normalized_host}:{port}"),
        None => format!("{scheme}://{normalized_host}"),
    })
}

fn canonical_jwk(jwk: &HostPublicJwk) -> Result<Vec<u8>, HostIdentityError> {
    let fields = BTreeMap::<&str, Value>::from([
        ("crv", json!(jwk.crv)),
        ("ext", json!(jwk.ext)),
        ("key_ops", json!(jwk.key_ops)),
        ("kty", json!(jwk.kty)),
        ("x", json!(jwk.x)),
    ]);
    serde_json::to_vec(&fields).map_err(HostIdentityError::Json)
}

#[cfg(test)]
mod tests {
    use agentsassemble_persistence::SqliteStore;
    use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
    use ring::signature::{ED25519, UnparsedPublicKey};
    use serde_json::Value;

    use super::{
        CentralHostIdentity, REGISTRATION_CONTEXT, ServerChallengeError,
        normalize_server_identity_origin,
    };

    #[test]
    fn challenge_origin_preserves_safe_explicit_authority() {
        for (input, expected) in [
            (
                "https://HOME.trycloudflare.com:443/",
                "https://home.trycloudflare.com:443",
            ),
            ("http://127.0.0.1:8765/", "http://127.0.0.1:8765"),
            ("http://[::1]:8765", "http://[::1]:8765"),
        ] {
            assert_eq!(
                normalize_server_identity_origin(input),
                Ok(expected.to_owned())
            );
        }
        for input in [
            "http://public.example",
            "https://user:secret@home.trycloudflare.com",
            "https://home.trycloudflare.com/path",
            "https://home.trycloudflare.com/?token=secret",
        ] {
            assert_eq!(
                normalize_server_identity_origin(input),
                Err(ServerChallengeError::InvalidOrigin)
            );
        }
    }

    #[tokio::test]
    async fn registration_envelope_matches_the_central_worker_transcript() {
        let store = SqliteStore::open("sqlite::memory:")
            .await
            .unwrap_or_else(|error| panic!("open authority: {error}"));
        let persistent = store
            .host_identity()
            .await
            .unwrap_or_else(|error| panic!("load host identity: {error}"));
        let identity = CentralHostIdentity::from_persistent(&persistent)
            .unwrap_or_else(|error| panic!("derive host identity: {error}"));
        let owner = "per_central-owner_123456";
        let envelope = identity
            .registration_envelope(owner)
            .unwrap_or_else(|error| panic!("create registration proof: {error}"));

        let public_key = URL_SAFE_NO_PAD
            .decode(&envelope.host_public_key_jwk.x)
            .unwrap_or_else(|error| panic!("decode public key: {error}"));
        let signature = URL_SAFE_NO_PAD
            .decode(&envelope.host_registration_proof.signature)
            .unwrap_or_else(|error| panic!("decode signature: {error}"));
        let transcript = format!(
            "{REGISTRATION_CONTEXT}\n{}\n{}\n{}\n{}",
            envelope.server_id,
            envelope.host_registration_proof.owner_person_id,
            envelope.host_registration_proof.issued_at,
            envelope.host_registration_proof.nonce
        );
        UnparsedPublicKey::new(&ED25519, &public_key)
            .verify(transcript.as_bytes(), &signature)
            .unwrap_or_else(|_| panic!("registration signature did not verify"));
        let substituted = transcript.replace(owner, "per_substituted_123456");
        assert!(
            UnparsedPublicKey::new(&ED25519, &public_key)
                .verify(substituted.as_bytes(), &signature)
                .is_err()
        );

        let payload = serde_json::to_value(&envelope)
            .unwrap_or_else(|error| panic!("serialize envelope: {error}"));
        let Value::Object(payload) = payload else {
            panic!("registration envelope is not an object");
        };
        assert_eq!(
            payload.keys().map(String::as_str).collect::<Vec<_>>(),
            [
                "host_key_fingerprint",
                "host_public_key_jwk",
                "host_registration_proof",
                "server_id",
            ]
        );
    }

    #[tokio::test]
    async fn stable_host_identity_uses_fresh_registration_nonces() {
        let store = SqliteStore::open("sqlite::memory:")
            .await
            .unwrap_or_else(|error| panic!("open authority: {error}"));
        let persistent = store
            .host_identity()
            .await
            .unwrap_or_else(|error| panic!("load host identity: {error}"));
        let identity = CentralHostIdentity::from_persistent(&persistent)
            .unwrap_or_else(|error| panic!("derive host identity: {error}"));
        let first = identity
            .registration_envelope("per_owner_12345678")
            .unwrap_or_else(|error| panic!("first proof: {error}"));
        let second = identity
            .registration_envelope("per_owner_12345678")
            .unwrap_or_else(|error| panic!("second proof: {error}"));

        assert_eq!(first.server_id, second.server_id);
        assert_eq!(first.host_public_key_jwk.x, second.host_public_key_jwk.x);
        assert_eq!(first.host_key_fingerprint, second.host_key_fingerprint);
        assert_ne!(
            first.host_registration_proof.nonce,
            second.host_registration_proof.nonce
        );
    }
}
