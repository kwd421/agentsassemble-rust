use std::fmt::Write;

use hmac::{Hmac, Mac};
use serde_json::Value;
use sha2::{Digest, Sha256};

pub fn expected_subscription_proof(proof_key: &str, receipt: &Value) -> String {
    let mut signer = Hmac::<Sha256>::new_from_slice(proof_key.as_bytes())
        .unwrap_or_else(|error| panic!("construct proof signer: {error}"));
    add_mac_field(&mut signer, b"agentsassemble.subscription-proof.v1");
    for field in [
        string(&receipt["server_challenge"]),
        string(&receipt["connection_nonce"]),
        string(&receipt["room_id"]),
        string(&receipt["principal_id"]),
        string(&receipt["participant_id"]),
        receipt["protocol_version"]
            .as_u64()
            .unwrap_or_default()
            .to_string(),
        "streams".to_owned(),
        string(&receipt["streams"][0]),
        receipt["server_surface_revision"]
            .as_u64()
            .unwrap_or_default()
            .to_string(),
        string(&receipt["server_surface_digest"]),
        string(&receipt["permissions_digest"]),
        receipt["snapshot_cursor"]
            .as_i64()
            .unwrap_or_default()
            .to_string(),
        receipt["catchup_high_water"]
            .as_i64()
            .unwrap_or_default()
            .to_string(),
        string(&receipt["snapshot_digest"]),
    ] {
        add_mac_field(&mut signer, field.as_bytes());
    }
    hex(&signer.finalize().into_bytes())
}

pub fn connection_nonce_for_ticket(ticket: &str) -> String {
    sha_transcript(
        "agentsassemble.ws-connection-nonce.v1",
        &[ticket.to_owned()],
    )
}

pub fn permissions_digest(capabilities: &Value) -> String {
    let keys = [
        "agent.control",
        "bridge.publish",
        "bridge.report",
        "message.modify",
        "message.send",
        "participant.kick",
        "participant.leave",
        "participant.mute",
        "provider.request.resolve",
        "room.delete",
        "room.history",
        "room.manage",
        "room.random",
        "room.vote.summary",
    ];
    let fields = keys
        .iter()
        .map(|key| {
            format!(
                "{key}={}",
                u8::from(capabilities[*key].as_bool().unwrap_or(false))
            )
        })
        .collect::<Vec<_>>();
    sha_transcript("agentsassemble.permissions.v1", &fields)
}

pub fn sha256_hex(value: &[u8]) -> String {
    hex(&Sha256::digest(value))
}

fn string(value: &Value) -> String {
    value.as_str().unwrap_or_default().to_owned()
}

fn sha_transcript(context: &str, fields: &[String]) -> String {
    let mut digest = Sha256::new();
    add_digest_field(&mut digest, context.as_bytes());
    for field in fields {
        add_digest_field(&mut digest, field.as_bytes());
    }
    hex(&digest.finalize())
}

fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .fold(String::with_capacity(64), |mut encoded, byte| {
            write!(encoded, "{byte:02x}")
                .unwrap_or_else(|error| panic!("encode proof byte: {error}"));
            encoded
        })
}

fn add_mac_field(signer: &mut Hmac<Sha256>, field: &[u8]) {
    signer.update(&u64::try_from(field.len()).unwrap_or(u64::MAX).to_be_bytes());
    signer.update(field);
}

fn add_digest_field(digest: &mut Sha256, field: &[u8]) {
    Digest::update(
        digest,
        u64::try_from(field.len()).unwrap_or(u64::MAX).to_be_bytes(),
    );
    Digest::update(digest, field);
}
