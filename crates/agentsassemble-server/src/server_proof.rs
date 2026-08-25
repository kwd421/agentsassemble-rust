use std::fmt::Write;

use agentsassemble_domain::CapabilitySet;
use agentsassemble_protocol::Subscribed;
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

const CONNECTION_NONCE_CONTEXT: &str = "agentsassemble.ws-connection-nonce.v1";
const PERMISSIONS_CONTEXT: &str = "agentsassemble.permissions.v1";
const SUBSCRIPTION_PROOF_CONTEXT: &str = "agentsassemble.subscription-proof.v1";
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

pub(crate) fn challenge_is_valid(challenge: &str) -> bool {
    challenge.len() == 64 && challenge.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub(crate) fn derive_connection_nonce(ticket: &str) -> String {
    hash_transcript(CONNECTION_NONCE_CONTEXT, [ticket])
}

pub(crate) fn snapshot_digest(encoded_snapshot: &str) -> String {
    encode_hex(Sha256::digest(encoded_snapshot.as_bytes()).as_slice())
}

pub(crate) fn permissions_digest(capabilities: &CapabilitySet) -> String {
    hash_transcript(
        PERMISSIONS_CONTEXT,
        [
            permission("agent.control", capabilities.agent_control),
            permission("bridge.publish", capabilities.bridge_publish),
            permission("bridge.report", capabilities.bridge_report),
            permission("message.modify", capabilities.message_modify),
            permission("message.send", capabilities.message_send),
            permission("participant.kick", capabilities.participant_kick),
            permission("participant.leave", capabilities.participant_leave),
            permission("participant.mute", capabilities.participant_mute),
            permission(
                "provider.request.resolve",
                capabilities.provider_request_resolve,
            ),
            permission("room.delete", capabilities.room_delete),
            permission("room.history", capabilities.room_history),
            permission("room.manage", capabilities.room_manage),
            permission("room.random", capabilities.room_random),
            permission("room.vote.summary", capabilities.room_vote_summary),
        ],
    )
}

pub(crate) fn sign_subscription(proof_key: &str, receipt: &Subscribed) -> String {
    let mut signer = Hmac::<Sha256>::new_from_slice(proof_key.as_bytes())
        .unwrap_or_else(|_| unreachable!("HMAC accepts keys of every length"));
    add_field(&mut signer, SUBSCRIPTION_PROOF_CONTEXT.as_bytes());
    for field in subscription_fields(receipt) {
        add_field(&mut signer, field.as_bytes());
    }
    encode_hex(&signer.finalize().into_bytes())
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

fn subscription_fields(receipt: &Subscribed) -> Vec<String> {
    let mut fields = vec![
        receipt.server_challenge.clone(),
        receipt.connection_nonce.clone(),
        receipt.room_id.clone(),
        receipt.principal_id.clone(),
        receipt.participant_id.clone(),
        receipt.protocol_version.to_string(),
        "streams".to_owned(),
    ];
    let mut streams = receipt.streams.clone();
    streams.sort();
    fields.extend(streams.iter().map(|stream| stream.as_str().to_owned()));
    fields.extend([
        receipt.server_surface_revision.to_string(),
        receipt.server_surface_digest.clone(),
        receipt.permissions_digest.clone(),
        receipt.snapshot_cursor.to_string(),
        receipt.catchup_high_water.to_string(),
        receipt.snapshot_digest.clone(),
    ]);
    fields
}

fn permission(key: &str, allowed: bool) -> String {
    format!("{key}={}", u8::from(allowed))
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
    use agentsassemble_domain::{CapabilitySet, ClientKind, InviteScope};
    use agentsassemble_protocol::{PROTOCOL_VERSION, RoomStream, Subscribed};

    use super::{
        FrameDirection, challenge_is_valid, derive_connection_nonce, permissions_digest,
        sign_frame, sign_subscription, verify_frame_proof,
    };

    fn receipt() -> Subscribed {
        Subscribed {
            streams: vec![RoomStream::RoomEvents],
            protocol_version: PROTOCOL_VERSION,
            server_challenge: "a".repeat(64),
            connection_nonce: derive_connection_nonce(&"b".repeat(64)),
            room_id: "general".to_owned(),
            principal_id: "operator".to_owned(),
            participant_id: "operator-local".to_owned(),
            server_surface_revision: 3,
            server_surface_digest: "c".repeat(64),
            permissions_digest: permissions_digest(&CapabilitySet::local_operator(
                ClientKind::Browser,
                InviteScope::ReadWrite,
            )),
            snapshot_cursor: 7,
            catchup_high_water: 9,
            snapshot_digest: "d".repeat(64),
            proof: String::new(),
        }
    }

    #[test]
    fn proof_binds_every_subscription_boundary() {
        let original = receipt();
        assert!(challenge_is_valid(&original.server_challenge));
        assert!(!challenge_is_valid("short"));
        let signature = sign_subscription("proof-key", &original);
        let mut changed = original;
        changed.catchup_high_water += 1;
        assert_ne!(signature, sign_subscription("proof-key", &changed));
    }

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
