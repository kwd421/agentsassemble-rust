use std::{collections::BTreeMap, sync::Arc};

use agentsassemble_persistence::PersistentHostIdentity;
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

#[derive(Debug, Error)]
pub enum HostIdentityError {
    #[error("persistent Ed25519 host seed is invalid")]
    InvalidSeed,
    #[error("host identity JSON projection failed")]
    Json(#[source] serde_json::Error),
    #[error("host registration entropy source failed")]
    Entropy,
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

#[derive(Clone)]
pub struct CentralHostIdentity {
    server_id: Arc<str>,
    key_pair: Arc<Ed25519KeyPair>,
    public_jwk: HostPublicJwk,
    fingerprint: Arc<str>,
}

impl CentralHostIdentity {
    /// Builds the public signing projection from the database-owned private seed.
    ///
    /// # Errors
    ///
    /// Rejects invalid Ed25519 material or an unserializable public projection.
    pub fn from_persistent(identity: &PersistentHostIdentity) -> Result<Self, HostIdentityError> {
        let key_pair = Ed25519KeyPair::from_seed_unchecked(identity.private_key_seed())
            .map_err(|_| HostIdentityError::InvalidSeed)?;
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
