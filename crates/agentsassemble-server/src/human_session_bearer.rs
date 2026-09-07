use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};

const BEARER_PREFIX: &str = "aas1.";
const BEARER_BYTES: usize = 32;
const BEARER_CHARS: usize = 48;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PresentedHumanSessionBearer {
    Other,
    Invalid,
    Fingerprint([u8; 32]),
}

pub(crate) fn classify_presented_bearer(value: &str) -> PresentedHumanSessionBearer {
    if !value.starts_with(BEARER_PREFIX) {
        return PresentedHumanSessionBearer::Other;
    }
    fingerprint_presented_bearer(value).map_or(
        PresentedHumanSessionBearer::Invalid,
        PresentedHumanSessionBearer::Fingerprint,
    )
}

pub(crate) fn fingerprint_presented_bearer(value: &str) -> Option<[u8; 32]> {
    if value.len() != BEARER_CHARS {
        return None;
    }
    let encoded = value.strip_prefix(BEARER_PREFIX)?;
    // The strict URL-safe engine rejects padding, other alphabets, and nonzero trailing bits.
    let decoded = URL_SAFE_NO_PAD.decode(encoded).ok()?;
    if decoded.len() != BEARER_BYTES {
        return None;
    }
    Some(fingerprint(value.as_bytes()))
}

fn fingerprint(value: &[u8]) -> [u8; 32] {
    Sha256::digest(value).into()
}

#[cfg(test)]
mod tests {
    use super::{PresentedHumanSessionBearer, classify_presented_bearer};

    #[test]
    fn presented_bearer_distinguishes_other_invalid_and_valid_credentials() {
        let valid = "aas1.azzIr-3RAkGakKN9P6yud8kvdUIp5QWcLJ3m_yDTqk4";
        assert!(matches!(
            classify_presented_bearer(valid),
            PresentedHumanSessionBearer::Fingerprint(_)
        ));
        assert_eq!(
            classify_presented_bearer("aas1.malformed"),
            PresentedHumanSessionBearer::Invalid
        );
        assert_eq!(
            classify_presented_bearer("0123456789abcdef"),
            PresentedHumanSessionBearer::Other
        );
    }
}
