use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};

pub(crate) const BROWSER_CREDENTIAL_PREFIX: &str = "aad1_";
const CREDENTIAL_BYTES: usize = 32;
const CREDENTIAL_CHARS: usize = 48;

/// Parses the one canonical browser credential domain and returns only its fingerprint.
pub(crate) fn fingerprint_browser_credential(value: &str) -> Option<[u8; 32]> {
    if value.len() != CREDENTIAL_CHARS {
        return None;
    }
    let encoded = value.strip_prefix(BROWSER_CREDENTIAL_PREFIX)?;
    // The strict URL-safe engine rejects padding, other alphabets, and nonzero trailing bits.
    let decoded = URL_SAFE_NO_PAD.decode(encoded).ok()?;
    if decoded.len() != CREDENTIAL_BYTES {
        return None;
    }
    Some(Sha256::digest(value.as_bytes()).into())
}
