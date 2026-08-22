use std::fmt::Write;

use hmac::{Hmac, Mac};
use sha2::Sha256;

const PROOF_CONTEXT: &str = "agentsassemble-server-proof-v1\0";

pub(crate) fn challenge_is_valid(challenge: &str) -> bool {
    challenge.len() == 64 && challenge.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub(crate) fn sign_challenge(proof_key: &str, challenge: &str) -> String {
    let mut signer = Hmac::<Sha256>::new_from_slice(proof_key.as_bytes())
        .unwrap_or_else(|_| unreachable!("HMAC accepts keys of every length"));
    signer.update(PROOF_CONTEXT.as_bytes());
    signer.update(challenge.as_bytes());
    encode_signature(signer)
}

fn encode_signature(signer: Hmac<Sha256>) -> String {
    signer
        .finalize()
        .into_bytes()
        .iter()
        .fold(String::with_capacity(64), |mut encoded, byte| {
            let _ = write!(encoded, "{byte:02x}");
            encoded
        })
}

#[cfg(test)]
mod tests {
    use super::{challenge_is_valid, sign_challenge};

    #[test]
    fn proof_is_bound_to_a_well_formed_unique_challenge() {
        let first = "a".repeat(64);
        let second = "b".repeat(64);
        assert!(challenge_is_valid(&first));
        assert!(!challenge_is_valid("short"));
        assert_ne!(
            sign_challenge("proof-key", &first),
            sign_challenge("proof-key", &second)
        );
    }
}
