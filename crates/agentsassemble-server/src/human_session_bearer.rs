use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};

const BEARER_PREFIX: &str = "aas1.";
const BEARER_BYTES: usize = 32;
const BEARER_BODY_CHARS: usize = 43;
const BEARER_CHARS: usize = 48;

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
    use super::fingerprint_presented_bearer;

    #[test]
    fn presented_bearer_requires_one_canonical_domain() {
        let valid = "aas1.azzIr-3RAkGakKN9P6yud8kvdUIp5QWcLJ3m_yDTqk4";
        let body = valid
            .strip_prefix("aas1.")
            .unwrap_or_else(|| panic!("fixed bearer prefix"));
        for malformed in [
            String::new(),
            format!("aas1.{}", &body[..42]),
            format!("aas1.{body}="),
            format!(" {valid}"),
            format!("aad1_{body}"),
        ] {
            assert_eq!(fingerprint_presented_bearer(&malformed), None);
        }
        assert!(fingerprint_presented_bearer(valid).is_some());
    }
}
