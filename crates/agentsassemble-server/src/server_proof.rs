use std::fmt::Write;

use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

const CONNECTION_NONCE_CONTEXT: &str = "agentsassemble.ws-connection-nonce.v1";
const FRAME_KEY_CONTEXT: &str = "agentsassemble.ws-frame-key.v1";
const FRAME_PROOF_CONTEXT: &str = "agentsassemble.ws-frame-proof.v1";

#[derive(Clone, Copy)]
pub(crate) enum FrameDirection {
    Client,
    Server,
}

impl FrameDirection {
    fn as_str(self) -> &'static str {
        match self {
            Self::Client => "client",
            Self::Server => "server",
        }
    }
}

pub(crate) fn derive_connection_nonce(ticket: &str) -> String {
    hash_transcript(CONNECTION_NONCE_CONTEXT, [ticket])
}

pub(crate) fn sign_frame(
    proof_key: &str,
    connection_nonce: &str,
    direction: FrameDirection,
    counter: u64,
    payload: &[u8],
) -> String {
    let connection_key = derive_frame_key(proof_key, connection_nonce);
    let mut signer = Hmac::<Sha256>::new_from_slice(&connection_key)
        .unwrap_or_else(|_| unreachable!("HMAC accepts keys of every length"));
    add_field(&mut signer, FRAME_PROOF_CONTEXT.as_bytes());
    add_field(&mut signer, connection_nonce.as_bytes());
    add_field(&mut signer, direction.as_str().as_bytes());
    add_field(&mut signer, counter.to_string().as_bytes());
    add_field(&mut signer, payload);
    encode_hex(&signer.finalize().into_bytes())
}

pub(crate) fn verify_frame_proof(
    proof_key: &str,
    connection_nonce: &str,
    direction: FrameDirection,
    counter: u64,
    payload: &[u8],
    proof: &str,
) -> bool {
    proof.len() == 64
        && proof
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        && bool::from(
            sign_frame(proof_key, connection_nonce, direction, counter, payload)
                .as_bytes()
                .ct_eq(proof.as_bytes()),
        )
}

fn derive_frame_key(proof_key: &str, connection_nonce: &str) -> [u8; 32] {
    let mut signer = Hmac::<Sha256>::new_from_slice(proof_key.as_bytes())
        .unwrap_or_else(|_| unreachable!("HMAC accepts keys of every length"));
    add_field(&mut signer, FRAME_KEY_CONTEXT.as_bytes());
    add_field(&mut signer, connection_nonce.as_bytes());
    signer.finalize().into_bytes().into()
}

fn hash_transcript<I, S>(context: &str, fields: I) -> String
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut digest = Sha256::new();
    add_field(&mut digest, context.as_bytes());
    for field in fields {
        add_field(&mut digest, field.as_ref().as_bytes());
    }
    encode_hex(&digest.finalize())
}

fn add_field(target: &mut impl MacOrDigest, value: &[u8]) {
    let length = u64::try_from(value.len())
        .unwrap_or_else(|_| panic!("subscription proof field exceeds u64"));
    target.update_bytes(&length.to_be_bytes());
    target.update_bytes(value);
}

trait MacOrDigest {
    fn update_bytes(&mut self, value: &[u8]);
}

impl MacOrDigest for Sha256 {
    fn update_bytes(&mut self, value: &[u8]) {
        Digest::update(self, value);
    }
}

impl MacOrDigest for Hmac<Sha256> {
    fn update_bytes(&mut self, value: &[u8]) {
        Mac::update(self, value);
    }
}

fn encode_hex(bytes: &[u8]) -> String {
    bytes.iter().fold(
        String::with_capacity(bytes.len() * 2),
        |mut encoded, byte| {
            let _ = write!(encoded, "{byte:02x}");
            encoded
        },
    )
}

#[cfg(test)]
mod tests {
    use super::{FrameDirection, derive_connection_nonce, sign_frame, verify_frame_proof};

    #[test]
    fn connection_nonce_is_ticket_bound() {
        assert_ne!(
            derive_connection_nonce(&"a".repeat(64)),
            derive_connection_nonce(&"b".repeat(64))
        );
    }

    #[test]
    fn frame_proof_binds_connection_direction_counter_and_exact_bytes() {
        let proof_key = "b".repeat(64);
        let nonce = derive_connection_nonce(&"c".repeat(64));
        let payload = br#"{"op":"ping","nonce":"exact"}"#;
        let proof = sign_frame(&proof_key, &nonce, FrameDirection::Client, 7, payload);
        assert!(verify_frame_proof(
            &proof_key,
            &nonce,
            FrameDirection::Client,
            7,
            payload,
            &proof,
        ));
        assert!(!verify_frame_proof(
            &proof_key,
            &nonce,
            FrameDirection::Server,
            7,
            payload,
            &proof,
        ));
        assert!(!verify_frame_proof(
            &proof_key,
            &nonce,
            FrameDirection::Client,
            8,
            payload,
            &proof,
        ));
        assert!(!verify_frame_proof(
            &proof_key,
            &nonce,
            FrameDirection::Client,
            7,
            br#"{"op":"ping","nonce":"changed"}"#,
            &proof,
        ));
    }
}
