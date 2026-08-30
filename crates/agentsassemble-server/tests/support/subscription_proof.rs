#![allow(dead_code)] // Each integration binary exercises a different subset of this shared peer.

use std::{fmt::Write, time::Duration};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use futures_util::{SinkExt, StreamExt};
use hmac::{Hmac, Mac};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio_tungstenite::{WebSocketStream, tungstenite::Message};

const FRAME_KEY_CONTEXT: &str = "agentsassemble.ws-frame-key.v1";
const FRAME_PROOF_CONTEXT: &str = "agentsassemble.ws-frame-proof.v1";

pub struct AuthenticatedTestSocket<S> {
    socket: WebSocketStream<S>,
    ticket: String,
    proof_key: String,
    connection_nonce: Option<String>,
    snapshot_pending: bool,
    next_client_counter: u64,
    next_server_counter: u64,
}

impl<S> AuthenticatedTestSocket<S>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    pub fn new(socket: WebSocketStream<S>, ticket: String, proof_key: String) -> Self {
        Self {
            socket,
            ticket,
            proof_key,
            connection_nonce: None,
            snapshot_pending: false,
            next_client_counter: 1,
            next_server_counter: 1,
        }
    }

    pub async fn subscribe(&mut self, cursor: i64) -> Value {
        let challenge = "d".repeat(64);
        self.send_plain(&json!({
            "op": "subscribe",
            "streams": ["room_events"],
            "resume_from_seq": cursor,
            "server_challenge": challenge,
        }))
        .await;
        let receipt = parse_json(&receive_wire_text(&mut self.socket).await);
        if receipt["op"] == "subscribed" {
            assert_eq!(receipt["server_challenge"], challenge);
            let nonce = connection_nonce_for_ticket(&self.ticket);
            assert_eq!(receipt["connection_nonce"], nonce);
            self.connection_nonce = Some(nonce);
            self.snapshot_pending = true;
        }
        receipt
    }

    pub async fn send_json(&mut self, frame: &Value) {
        let payload = frame.to_string();
        let nonce = self
            .connection_nonce
            .as_deref()
            .unwrap_or_else(|| panic!("authenticated test socket has not subscribed"));
        let counter = self.next_client_counter;
        let envelope = authenticated_envelope(
            &self.proof_key,
            nonce,
            "client",
            counter,
            payload.as_bytes(),
        );
        self.socket
            .send(Message::Text(envelope.to_string().into()))
            .await
            .unwrap_or_else(|error| panic!("send authenticated test frame: {error}"));
        self.next_client_counter += 1;
    }

    pub async fn send_tampered_json(&mut self, signed: &Value, transmitted: &Value) {
        let signed_payload = signed.to_string();
        let transmitted_payload = transmitted.to_string();
        let nonce = self
            .connection_nonce
            .as_deref()
            .unwrap_or_else(|| panic!("authenticated test socket has not subscribed"));
        let counter = self.next_client_counter;
        let mut envelope = authenticated_envelope(
            &self.proof_key,
            nonce,
            "client",
            counter,
            signed_payload.as_bytes(),
        );
        envelope["payload"] = Value::String(STANDARD.encode(transmitted_payload.as_bytes()));
        self.socket
            .send(Message::Text(envelope.to_string().into()))
            .await
            .unwrap_or_else(|error| panic!("send tampered test frame: {error}"));
        self.next_client_counter += 1;
    }

    pub async fn receive_json(&mut self) -> Value {
        parse_json(&self.receive_text().await)
    }

    pub async fn receive_json_with_timeout(&mut self, timeout: Duration) -> Value {
        parse_json(&self.receive_text_with_timeout(timeout).await)
    }

    pub async fn receive_text(&mut self) -> String {
        let raw = receive_wire_text(&mut self.socket).await;
        self.authenticate_received_text(raw)
    }

    async fn receive_text_with_timeout(&mut self, timeout: Duration) -> String {
        let raw = receive_wire_text_with_timeout(&mut self.socket, timeout).await;
        self.authenticate_received_text(raw)
    }

    fn authenticate_received_text(&mut self, raw: String) -> String {
        if self.snapshot_pending {
            self.snapshot_pending = false;
            return raw;
        }
        let Some(nonce) = self.connection_nonce.as_deref() else {
            return raw;
        };
        let envelope = parse_json(&raw);
        assert_eq!(envelope["op"], "authenticated");
        let counter = envelope["counter"]
            .as_u64()
            .unwrap_or_else(|| panic!("authenticated server frame omitted its counter"));
        assert_eq!(counter, self.next_server_counter);
        let encoded = envelope["payload"]
            .as_str()
            .unwrap_or_else(|| panic!("authenticated server frame omitted its payload"));
        let payload = STANDARD
            .decode(encoded)
            .unwrap_or_else(|error| panic!("decode authenticated server payload: {error}"));
        assert_eq!(STANDARD.encode(&payload), encoded);
        let received = envelope["proof"]
            .as_str()
            .unwrap_or_else(|| panic!("authenticated server frame omitted its proof"));
        assert_eq!(
            received,
            expected_frame_proof(&self.proof_key, nonce, "server", counter, &payload,)
        );
        self.next_server_counter += 1;
        String::from_utf8(payload)
            .unwrap_or_else(|error| panic!("authenticated server payload is not UTF-8: {error}"))
    }

    pub async fn send_binary(&mut self, bytes: Vec<u8>) {
        self.socket
            .send(Message::Binary(bytes.into()))
            .await
            .unwrap_or_else(|error| panic!("send binary test frame: {error}"));
    }

    pub async fn close(&mut self) {
        self.socket
            .close(None)
            .await
            .unwrap_or_else(|error| panic!("close authenticated test socket: {error}"));
    }

    pub async fn wait_closed(&mut self) -> bool {
        let closed = tokio::time::timeout(Duration::from_secs(1), self.socket.next()).await;
        matches!(closed, Ok(None | Some(Ok(Message::Close(_)) | Err(_))))
    }

    pub async fn has_no_frame_for(&mut self, duration: Duration) -> bool {
        tokio::time::timeout(duration, self.socket.next())
            .await
            .is_err()
    }

    async fn send_plain(&mut self, frame: &Value) {
        self.socket
            .send(Message::Text(frame.to_string().into()))
            .await
            .unwrap_or_else(|error| panic!("send plain test frame: {error}"));
    }
}

pub fn expected_frame_proof(
    proof_key: &str,
    connection_nonce: &str,
    direction: &str,
    counter: u64,
    payload: &[u8],
) -> String {
    let connection_key = frame_key(proof_key, connection_nonce);
    let mut signer = Hmac::<Sha256>::new_from_slice(&connection_key)
        .unwrap_or_else(|error| panic!("construct frame signer: {error}"));
    add_mac_field(&mut signer, FRAME_PROOF_CONTEXT.as_bytes());
    add_mac_field(&mut signer, connection_nonce.as_bytes());
    add_mac_field(&mut signer, direction.as_bytes());
    add_mac_field(&mut signer, counter.to_string().as_bytes());
    add_mac_field(&mut signer, payload);
    hex(&signer.finalize().into_bytes())
}

fn authenticated_envelope(
    proof_key: &str,
    connection_nonce: &str,
    direction: &str,
    counter: u64,
    payload: &[u8],
) -> Value {
    json!({
        "op": "authenticated",
        "counter": counter,
        "payload": STANDARD.encode(payload),
        "proof": expected_frame_proof(
            proof_key,
            connection_nonce,
            direction,
            counter,
            payload,
        ),
    })
}

fn frame_key(proof_key: &str, connection_nonce: &str) -> [u8; 32] {
    let mut signer = Hmac::<Sha256>::new_from_slice(proof_key.as_bytes())
        .unwrap_or_else(|error| panic!("construct frame key signer: {error}"));
    add_mac_field(&mut signer, FRAME_KEY_CONTEXT.as_bytes());
    add_mac_field(&mut signer, connection_nonce.as_bytes());
    signer.finalize().into_bytes().into()
}

fn parse_json(raw: &str) -> Value {
    serde_json::from_str(raw).unwrap_or_else(|error| panic!("decode WebSocket JSON: {error}"))
}

async fn receive_wire_text<S>(socket: &mut WebSocketStream<S>) -> String
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let message = socket
        .next()
        .await
        .unwrap_or_else(|| panic!("WebSocket closed before the expected frame"))
        .unwrap_or_else(|error| panic!("receive WebSocket frame: {error}"));
    String::from_utf8(message.into_data().to_vec())
        .unwrap_or_else(|error| panic!("WebSocket JSON is not UTF-8: {error}"))
}

async fn receive_wire_text_with_timeout<S>(
    socket: &mut WebSocketStream<S>,
    timeout: Duration,
) -> String
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let message = tokio::time::timeout(timeout, socket.next())
        .await
        .unwrap_or_else(|_| panic!("timed out waiting for WebSocket frame"))
        .unwrap_or_else(|| panic!("WebSocket closed before the expected frame"))
        .unwrap_or_else(|error| panic!("receive WebSocket frame: {error}"));
    String::from_utf8(message.into_data().to_vec())
        .unwrap_or_else(|error| panic!("WebSocket JSON is not UTF-8: {error}"))
}

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
