#![allow(dead_code)] // Integration binaries exercise different local-socket helpers.

use std::fmt::Write;

use hmac::{Hmac, Mac};
use reqwest::Client;
use serde_json::Value;
use sha2::Sha256;
use tokio_tungstenite::connect_async;

use super::subscription_proof::AuthenticatedTestSocket;

const HOST_CHALLENGE_CONTEXT: &str = "agentsassemble-host-challenge-v1\0";
const HOST_REQUEST_CONTEXT: &str = "agentsassemble-host-ticket-request-v1\0";
const HOST_RESPONSE_CONTEXT: &str = "agentsassemble-host-ticket-response-v1\0";

pub async fn connect(
    base_url: &str,
    host_token: &str,
    room_id: &str,
) -> AuthenticatedTestSocket<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>> {
    let grant = request_ticket(base_url, host_token, room_id).await;
    let ticket = grant["ticket"]
        .as_str()
        .unwrap_or_else(|| panic!("ticket response has no ticket"))
        .to_owned();
    let proof_key = grant["server_proof_key"]
        .as_str()
        .unwrap_or_else(|| panic!("ticket response has no proof key"))
        .to_owned();
    let url = format!(
        "{}/ws?ticket={ticket}",
        base_url.replacen("http://", "ws://", 1)
    );
    let socket = connect_async(url)
        .await
        .unwrap_or_else(|error| panic!("connect WebSocket: {error}"))
        .0;
    AuthenticatedTestSocket::new(socket, ticket, proof_key)
}

pub async fn request_ticket(base_url: &str, host_token: &str, room_id: &str) -> Value {
    let challenge = request_host_challenge(base_url, host_token).await;
    let proof = expected_host_request_proof(host_token, &challenge, room_id);
    let response = Client::new()
        .post(format!("{base_url}/api/ws-ticket"))
        .header("x-host-challenge", &challenge)
        .header("x-host-meeting", room_id)
        .header("x-host-proof", proof)
        .send()
        .await
        .unwrap_or_else(|error| panic!("request ticket: {error}"));
    assert!(response.status().is_success());
    let grant: Value = response
        .json()
        .await
        .unwrap_or_else(|error| panic!("decode ticket: {error}"));
    let ticket = grant["ticket"]
        .as_str()
        .unwrap_or_else(|| panic!("ticket response has no ticket"));
    let ttl_seconds = grant["ttl_seconds"]
        .as_u64()
        .unwrap_or_else(|| panic!("ticket response has no TTL"));
    let proof_key = grant["server_proof_key"]
        .as_str()
        .unwrap_or_else(|| panic!("ticket response has no proof key"));
    assert_eq!(
        grant["host_response_proof"],
        expected_host_response_proof(host_token, &challenge, ticket, ttl_seconds, proof_key,)
    );
    grant
}

pub async fn assert_ticket_challenge_is_single_use(
    base_url: &str,
    host_token: &str,
    room_id: &str,
) {
    let challenge = request_host_challenge(base_url, host_token).await;
    let proof = expected_host_request_proof(host_token, &challenge, room_id);
    let client = Client::new();
    for expected in [reqwest::StatusCode::OK, reqwest::StatusCode::UNAUTHORIZED] {
        let response = client
            .post(format!("{base_url}/api/ws-ticket"))
            .header("x-host-challenge", &challenge)
            .header("x-host-meeting", room_id)
            .header("x-host-proof", &proof)
            .send()
            .await
            .unwrap_or_else(|error| panic!("exercise single-use host challenge: {error}"));
        assert_eq!(response.status(), expected);
    }
}

pub async fn request_host_challenge(base_url: &str, host_token: &str) -> String {
    let response = Client::new()
        .get(format!("{base_url}/api/host-challenge"))
        .send()
        .await
        .unwrap_or_else(|error| panic!("request host challenge: {error}"));
    assert!(response.status().is_success());
    let grant: Value = response
        .json()
        .await
        .unwrap_or_else(|error| panic!("decode host challenge: {error}"));
    let challenge = grant["challenge"]
        .as_str()
        .unwrap_or_else(|| panic!("host challenge response has no challenge"));
    assert_eq!(
        grant["host_challenge_proof"],
        expected_hmac(host_token, HOST_CHALLENGE_CONTEXT, &[challenge])
    );
    challenge.to_owned()
}

pub fn expected_host_request_proof(secret: &str, challenge: &str, room_id: &str) -> String {
    expected_hmac(secret, HOST_REQUEST_CONTEXT, &[challenge, room_id])
}

fn expected_host_response_proof(
    secret: &str,
    challenge: &str,
    ticket: &str,
    ttl_seconds: u64,
    proof_key: &str,
) -> String {
    expected_hmac(
        secret,
        HOST_RESPONSE_CONTEXT,
        &[challenge, ticket, &ttl_seconds.to_string(), proof_key],
    )
}

fn expected_hmac(secret: &str, context: &str, fields: &[&str]) -> String {
    let mut signer = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
        .unwrap_or_else(|error| panic!("construct host proof signer: {error}"));
    signer.update(context.as_bytes());
    for field in fields {
        signer.update(field.as_bytes());
        signer.update(&[0]);
    }
    signer
        .finalize()
        .into_bytes()
        .iter()
        .fold(String::with_capacity(64), |mut encoded, byte| {
            write!(encoded, "{byte:02x}")
                .unwrap_or_else(|error| panic!("encode host proof: {error}"));
            encoded
        })
}
