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
    pub mod room_socket_peer;
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
        // Real socket I/O must not run while Tokio can auto-advance to the idle deadline.
        tokio::time::resume();
        socket
            .send_json(&json!({"op": "ping", "nonce": format!("keepalive-{nonce}")}))
            .await;
        let pong = socket.receive_json().await;
        assert_eq!(pong["op"], "pong");
        assert_eq!(pong["nonce"], format!("keepalive-{nonce}"));
        tokio::time::pause();
    }
    // Cross the durable one-hour deadline without reaching the independent
    // five-minute idle deadline measured from the last successful ping.
    tokio::time::advance(Duration::from_secs(270)).await;
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

#[tokio::test]
async fn paired_socket_retains_session_provenance_and_leave_preserves_native_host() {
    use agentsassemble_domain::{LOCAL_OPERATOR_PARTICIPANT_ID, LOCAL_OPERATOR_USER_ID};
    use agentsassemble_persistence::RoomSessionAuthorization;
    use support::room_socket_peer::RoomSocketPeer;
    let (store, _) = fixture(InviteScope::ReadWrite).await;
    let manager = store
        .authorize_local_room_manager(
            "general",
            LOCAL_OPERATOR_USER_ID,
            LOCAL_OPERATOR_PARTICIPANT_ID,
        )
        .await
        .unwrap_or_else(|error| panic!("authorize host: {error}"));
    let now = chrono::Utc::now();
    store
        .create_operator_pairing(&manager, &[0x51; 32], "https://room.example.test", now)
        .await
        .unwrap_or_else(|error| panic!("create pair: {error}"));
    let paired = store
        .redeem_operator_pairing(&[0x51; 32], &[0x52; 32], "https://room.example.test", now)
        .await
        .unwrap_or_else(|error| panic!("redeem pair: {error}"));
    let authorization = RoomSessionAuthorization::Operator(paired.authorization);
    let server = start(store.clone()).await;
    let mut grants = Vec::new();
    for _ in 0..2 {
        grants.push(
            server
                .state()
                .tickets
                .issue_room_session_socket(authorization.clone())
                .await
                .unwrap_or_else(|error| panic!("issue paired socket: {error}")),
        );
    }
    let endpoint = server.base_url.replace("http://", "ws://");
    let (wire, _) =
        tokio_tungstenite::connect_async(format!("{endpoint}/ws?ticket={}", grants[0].ticket))
            .await
            .unwrap_or_else(|error| panic!("connect paired socket: {error}"));
    let mut socket = RoomSocketPeer::new(wire);
    assert_eq!(socket.subscribe(0).await["op"], "subscribed");
    assert_eq!(socket.receive_json().await["op"], "snapshot");
    socket.send_json(&json!({"op": "command", "request_id": "paired-socket-leave", "action": "participant.leave", "payload": {}})).await;
    let ack = socket.receive_json().await;
    assert_eq!(ack["op"], "ack");
    assert_eq!(ack["accepted"], true);
    assert!(socket.wait_closed().await);
    assert!(
        store
            .revalidate_room_session_authorization(&authorization)
            .await
            .is_err()
    );
    assert!(
        store
            .resolve_principal(authorization.principal())
            .await
            .is_ok()
    );
    // A ticket issued before departure must not revive its now-revoked paired authority.
    let (wire, _) =
        tokio_tungstenite::connect_async(format!("{endpoint}/ws?ticket={}", grants[1].ticket))
            .await
            .unwrap_or_else(|error| panic!("connect stale paired ticket: {error}"));
    let mut stale = RoomSocketPeer::new(wire);
    assert!(stale.wait_closed().await);
    server.stop().await;
}
