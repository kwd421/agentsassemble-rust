use std::time::Duration;

use agentsassemble_domain::InviteScope;
use agentsassemble_persistence::{
    HumanAdmissionDecision, HumanAdmissionInput, HumanInviteCredentialEvidence,
    PreparedHumanAdmission,
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use reqwest::Client;
use serde_json::json;
use sha2::{Digest, Sha256};

mod support {
    pub mod human_invite;
    pub mod subscription_proof;
}

use support::human_invite::{
    canonical_session_token, fixture, fixture_with_max_uses, join, open_session_socket,
    persist_invite, start,
};

#[tokio::test]
async fn durable_session_deadline_closes_an_active_socket() {
    let (store, invite) = fixture(InviteScope::ReadWrite).await;
    let server = start(store).await;
    let client = Client::new();
    let browser_credential = format!("aad1_{}", URL_SAFE_NO_PAD.encode([0xE7; 32]));
    let admitted = join(
        &client,
        &server.base_url,
        invite.join_code(),
        &browser_credential,
        "523e4567-e89b-12d3-a456-426614174000",
        "Deadline Guest",
        "",
    )
    .await;
    let mut socket = open_session_socket(
        &client,
        &server.base_url,
        canonical_session_token(&admitted),
    )
    .await;

    tokio::time::pause();
    for nonce in 0..14 {
        tokio::time::advance(Duration::from_mins(4)).await;
        tokio::task::yield_now().await;
        socket
            .send_json(&json!({"op": "ping", "nonce": format!("keepalive-{nonce}")}))
            .await;
        tokio::time::resume();
        let pong = socket.receive_json().await;
        assert_eq!(pong["op"], "pong");
        assert_eq!(pong["nonce"], format!("keepalive-{nonce}"));
        tokio::time::pause();
    }
    tokio::time::advance(Duration::from_mins(5)).await;
    tokio::task::yield_now().await;
    tokio::time::resume();
    assert!(
        socket.wait_closed().await,
        "session socket stayed open past its durable expiry deadline"
    );
    server.stop().await;
}

#[tokio::test]
async fn missed_revocation_notification_cannot_leak_the_next_outbound_event() {
    let (store, first_invite) = fixture_with_max_uses(InviteScope::ReadWrite, 5).await;
    let second_invite = persist_invite(
        &store,
        InviteScope::ReadWrite,
        5,
        "outbound-race-guest",
        "Outbound Race Guest",
    )
    .await;
    let server = start(store.clone()).await;
    let client = Client::new();
    let browser_credential = format!("aad1_{}", URL_SAFE_NO_PAD.encode([0xF7; 32]));
    let first = join(
        &client,
        &server.base_url,
        first_invite.join_code(),
        &browser_credential,
        "623e4567-e89b-12d3-a456-426614174000",
        "First Outbound Guest",
        "",
    )
    .await;
    let mut socket =
        open_session_socket(&client, &server.base_url, canonical_session_token(&first)).await;

    let prepared = PreparedHumanAdmission::prepare(
        HumanInviteCredentialEvidence::JoinCode {
            fingerprint: *second_invite.join_code_fingerprint(),
        },
        Sha256::digest(browser_credential.as_bytes()).into(),
        &HumanAdmissionInput {
            request_id: "723e4567-e89b-12d3-a456-426614174000".to_owned(),
            meeting_id_assertion: "general".to_owned(),
            display_name: "Replacement Outbound Guest".to_owned(),
            participant_type: "human".to_owned(),
            owner_display_name: String::new(),
            client_id: "browser-boundary-client".to_owned(),
            avatar_image_url: String::new(),
        },
    )
    .unwrap_or_else(|error| panic!("prepare controlled replacement: {error}"));
    let replacement = match store
        .admit_human(&prepared, chrono::Utc::now())
        .await
        .unwrap_or_else(|error| panic!("commit controlled replacement: {error}"))
    {
        HumanAdmissionDecision::Admitted(commit) => commit,
        HumanAdmissionDecision::Rejected(rejection) => {
            panic!("controlled replacement was rejected: {rejection:?}")
        }
    };
    assert_eq!(replacement.replaced_session_fingerprints().len(), 1);
    assert!(!replacement.events().is_empty());

    // Publish the real durable event without the derived revocation broadcast. The
    // final outbound database check must still reject the displaced session.
    server
        .rooms()
        .notify_committed_events(replacement.events())
        .await;
    assert!(
        socket.wait_closed().await,
        "displaced socket received a post-replacement product frame"
    );
    server.stop().await;
}
