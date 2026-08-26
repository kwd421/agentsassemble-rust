use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};

pub(crate) const BROWSER_CREDENTIAL_PREFIX: &str = "aad1_";
const CREDENTIAL_BYTES: usize = 32;
const CREDENTIAL_BODY_CHARS: usize = 43;
const CREDENTIAL_CHARS: usize = 48;

/// Parses the one canonical browser credential domain and returns only its fingerprint.
pub(crate) fn fingerprint_browser_credential(value: &str) -> Option<[u8; 32]> {
    if value.len() != CREDENTIAL_CHARS || !value.is_ascii() {
        return None;
    }
    let encoded = value.strip_prefix(BROWSER_CREDENTIAL_PREFIX)?;
    if encoded.len() != CREDENTIAL_BODY_CHARS
        || !encoded
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return None;
    }
    let decoded = URL_SAFE_NO_PAD.decode(encoded).ok()?;
    if decoded.len() != CREDENTIAL_BYTES || URL_SAFE_NO_PAD.encode(decoded) != encoded {
        return None;
    }
    Some(Sha256::digest(value.as_bytes()).into())
}
