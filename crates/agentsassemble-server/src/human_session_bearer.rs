use std::sync::Arc;

use agentsassemble_persistence::PersistentHostIdentity;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

const BEARER_PREFIX: &str = "aas1.";
const BEARER_CONTEXT: &[u8] = b"agentsassemble-human-session-bearer-v1\0";
const BEARER_BYTES: usize = 32;
const BEARER_BODY_CHARS: usize = 43;
const BEARER_CHARS: usize = 48;

/// Issues restart-stable human room-session bearers from one durable admission key.
///
/// This type deliberately implements neither `Debug` nor serialization so the
/// private HMAC key cannot enter generic diagnostics or wire projections.
#[derive(Clone)]
pub struct HumanSessionBearerAuthority {
    key: Arc<[u8; 32]>,
}

impl HumanSessionBearerAuthority {
    #[must_use]
    pub fn from_persistent(identity: &PersistentHostIdentity) -> Self {
        Self {
            key: Arc::new(*identity.session_hmac_key()),
        }
    }

    /// Derives the only bearer authorized for one fixed durable admission key.
    #[must_use]
    pub fn issue(&self, admission_key: &[u8; 32]) -> IssuedHumanSessionBearer {
        let mut signer = Hmac::<Sha256>::new_from_slice(self.key.as_slice())
            .unwrap_or_else(|_| unreachable!("HMAC accepts a 32-byte key"));
        signer.update(BEARER_CONTEXT);
        signer.update(admission_key);
        let mac: [u8; BEARER_BYTES] = signer.finalize().into_bytes().into();
        let mut bearer = String::with_capacity(BEARER_CHARS);
        bearer.push_str(BEARER_PREFIX);
        URL_SAFE_NO_PAD.encode_string(mac, &mut bearer);
        let fingerprint = fingerprint(bearer.as_bytes());
        IssuedHumanSessionBearer {
            bearer,
            fingerprint,
        }
    }
}

/// One raw response credential and its non-secret durable lookup fingerprint.
///
/// This value deliberately implements neither `Debug` nor serialization.
pub struct IssuedHumanSessionBearer {
    bearer: String,
    fingerprint: [u8; 32],
}

impl IssuedHumanSessionBearer {
    #[must_use]
    pub fn bearer(&self) -> &str {
        &self.bearer
    }

    #[must_use]
    pub const fn fingerprint(&self) -> &[u8; 32] {
        &self.fingerprint
    }
}

pub(crate) fn fingerprint_presented_bearer(value: &str) -> Option<[u8; 32]> {
    if value.len() != BEARER_CHARS || !value.is_ascii() {
        return None;
    }
    let encoded = value.strip_prefix(BEARER_PREFIX)?;
    if encoded.len() != BEARER_BODY_CHARS
        || !encoded
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return None;
    }
    let decoded = URL_SAFE_NO_PAD.decode(encoded).ok()?;
    if decoded.len() != BEARER_BYTES || URL_SAFE_NO_PAD.encode(decoded) != encoded {
        return None;
    }
    Some(fingerprint(value.as_bytes()))
}

fn fingerprint(value: &[u8]) -> [u8; 32] {
    Sha256::digest(value).into()
}

#[cfg(test)]
mod tests {
    use super::{HumanSessionBearerAuthority, fingerprint_presented_bearer};

    fn authority() -> HumanSessionBearerAuthority {
        HumanSessionBearerAuthority {
            key: std::sync::Arc::new([0x11; 32]),
        }
    }

    #[test]
    fn fixed_vector_uses_the_complete_mac_and_bearer() {
        let issued = authority().issue(&[0x22; 32]);
        assert!(
            issued.bearer() == "aas1.azzIr-3RAkGakKN9P6yud8kvdUIp5QWcLJ3m_yDTqk4",
            "fixed bearer changed"
        );
        assert_eq!(
            issued.fingerprint(),
            &[
                0x3f, 0xfa, 0xdb, 0x80, 0xcb, 0xc3, 0x3f, 0x4b, 0x40, 0x90, 0x20, 0x7d, 0xea, 0x94,
                0x13, 0xa8, 0xf5, 0x0a, 0xc3, 0x9a, 0x07, 0x16, 0x0c, 0x0b, 0x9b, 0xd5, 0xdb, 0x45,
                0x21, 0xa3, 0xc8, 0x1f,
            ]
        );
        assert_eq!(
            fingerprint_presented_bearer(issued.bearer()),
            Some(*issued.fingerprint())
        );
        assert!(
            authority().issue(&[0x22; 32]).bearer() == issued.bearer(),
            "same authority and admission key changed bearer"
        );
        assert!(
            authority().issue(&[0x23; 32]).bearer() != issued.bearer(),
            "different admission keys shared one bearer"
        );
    }

    #[test]
    fn presented_bearer_requires_one_canonical_domain() {
        let valid = authority().issue(&[0x22; 32]);
        let body = valid
            .bearer()
            .strip_prefix("aas1.")
            .unwrap_or_else(|| panic!("fixed bearer prefix"));
        for malformed in [
            String::new(),
            format!("aas1.{}", &body[..42]),
            format!("aas1.{body}="),
            format!(" {0}", valid.bearer()),
            format!("aad1_{body}"),
        ] {
            assert_eq!(fingerprint_presented_bearer(&malformed), None);
        }
    }

    #[tokio::test]
    async fn persistent_host_recovers_the_same_bearer_after_reopen() {
        let directory = tempfile::tempdir().unwrap_or_else(|error| panic!("tempdir: {error}"));
        let path = directory.path().join("runtime.sqlite3");
        let first = agentsassemble_persistence::SqliteStore::open_path(&path)
            .await
            .unwrap_or_else(|error| panic!("create first host: {error}"));
        let first_identity = first
            .host_identity()
            .await
            .unwrap_or_else(|error| panic!("load first host identity: {error}"));
        let first_bearer =
            HumanSessionBearerAuthority::from_persistent(&first_identity).issue(&[0x44; 32]);
        drop(first_identity);
        drop(first);

        let reopened = agentsassemble_persistence::SqliteStore::open_path(&path)
            .await
            .unwrap_or_else(|error| panic!("reopen host: {error}"));
        let reopened_identity = reopened
            .host_identity()
            .await
            .unwrap_or_else(|error| panic!("load reopened host identity: {error}"));
        let reopened_bearer =
            HumanSessionBearerAuthority::from_persistent(&reopened_identity).issue(&[0x44; 32]);
        assert!(
            reopened_bearer.bearer() == first_bearer.bearer(),
            "reopen changed the deterministic bearer"
        );
        assert_eq!(reopened_bearer.fingerprint(), first_bearer.fingerprint());

        let other_directory =
            tempfile::tempdir().unwrap_or_else(|error| panic!("other tempdir: {error}"));
        let other = agentsassemble_persistence::SqliteStore::open_path(
            &other_directory.path().join("runtime.sqlite3"),
        )
        .await
        .unwrap_or_else(|error| panic!("create other host: {error}"));
        let other_identity = other
            .host_identity()
            .await
            .unwrap_or_else(|error| panic!("load other host identity: {error}"));
        let other_bearer =
            HumanSessionBearerAuthority::from_persistent(&other_identity).issue(&[0x44; 32]);
        assert!(
            other_bearer.bearer() != first_bearer.bearer(),
            "different hosts shared one deterministic bearer"
        );
    }
}
