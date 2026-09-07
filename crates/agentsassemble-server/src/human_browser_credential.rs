use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};

use agentsassemble_protocol::{
    BROWSER_CREDENTIAL_BYTES, BROWSER_CREDENTIAL_CHARS, BROWSER_CREDENTIAL_PREFIX,
};

/// Parses the one canonical browser credential domain and returns only its fingerprint.
pub(crate) fn fingerprint_browser_credential(value: &str) -> Option<[u8; 32]> {
    if value.len() != BROWSER_CREDENTIAL_CHARS {
        return None;
    }
    let encoded = value.strip_prefix(BROWSER_CREDENTIAL_PREFIX)?;
    // The strict URL-safe engine rejects padding, other alphabets, and nonzero trailing bits.
    let decoded = URL_SAFE_NO_PAD.decode(encoded).ok()?;
    if decoded.len() != BROWSER_CREDENTIAL_BYTES {
        return None;
    }
    Some(Sha256::digest(value.as_bytes()).into())
}
