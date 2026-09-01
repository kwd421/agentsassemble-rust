use agentsassemble_domain::{
    AuthenticatedPrincipal, CapabilitySet, ClientKind, InviteScope, LOCAL_OPERATOR_PARTICIPANT_ID,
    LOCAL_OPERATOR_USER_ID,
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use reqwest::Client;
use serde_json::json;
use sha2::{Digest, Sha256};

mod support {
    pub mod human_invite;
    pub mod local_socket;
    pub mod subscription_proof;
}

use support::{
    human_invite::{canonical_session_token, fixture, join, open_session_socket, start},
    local_socket::connect,
};

#[tokio::test]
async fn authenticated_tcp_summary_is_strict_private_read_only_and_revocable() {
    let (store, credentials) = fixture(InviteScope::ReadOnly).await;
    let (vote_id, durable_last_seq) = seed_vote(&store).await;
    let server = start(store.clone()).await;
    let local_tallies = assert_local_tcp_summary(
        &server.base_url,
        server.state(),
        &store,
        &vote_id,
        durable_last_seq,
    )
    .await;

    let client = Client::new();
    let admission = join(
        &client,
        &server.base_url,
        credentials.join_code(),
        &format!("aad1_{}", URL_SAFE_NO_PAD.encode([0x59; 32])),
        "423e4567-e89b-12d3-a456-426614174000",
        "Vote Reader",
        "",
    )
    .await;
    let session_token = canonical_session_token(&admission);
    let mut remote = open_session_socket(&client, &server.base_url, session_token).await;
    let admitted_last_seq = store
        .snapshot("general", 0, 1)
        .await
        .unwrap_or_else(|error| panic!("read high water after admission: {error}"))
        .last_seq;
    remote
        .send_json(&json!({
            "op": "command",
            "request_id": "read-only-vote-summary",
            "action": "room.vote.summary",
            "payload": {"vote_id": vote_id}
        }))
        .await;
    let remote_summary = remote.receive_json().await;
    assert_eq!(remote_summary["op"], "ack");
    assert_eq!(remote_summary["result"]["own_choice"], "");
    assert_eq!(remote_summary["result"]["tallies"], local_tallies);
    assert_eq!(
        store
            .snapshot("general", 0, 1)
            .await
            .unwrap_or_else(|error| panic!("read high water after summaries: {error}"))
            .last_seq,
        admitted_last_seq
    );

    let session_fingerprint: [u8; 32] = Sha256::digest(session_token.as_bytes()).into();
    let authorization = store
        .authorize_human_session(&session_fingerprint)
        .await
        .unwrap_or_else(|error| panic!("authorize vote reader for controlled revoke: {error}"));
    let revoked = store
        .execute_human_session_participant_leave(
            &authorization,
            "40000000-0000-4000-8000-000000000003",
            &json!({}),
        )
        .await
        .unwrap_or_else(|error| panic!("commit controlled vote reader revoke: {error}"));
    assert_eq!(revoked.revoked_session_fingerprints.len(), 1);
    // No derived revocation broadcast is published. The next real TCP frame must still hit the
    // durable session boundary and close without exposing another summary.
    remote
        .send_json(&json!({
            "op": "command",
            "request_id": "revoked-vote-summary",
            "action": "room.vote.summary",
            "payload": {"vote_id": vote_id}
        }))
        .await;
    assert!(remote.wait_closed().await);
    server.stop().await;
}

async fn seed_vote(store: &agentsassemble_persistence::SqliteStore) -> (String, i64) {
    let principal = local_principal();
    let created = store
        .execute_message_with_turn(
            &principal,
            "40000000-0000-4000-8000-000000000001",
            "message.send",
            &json!({
                "kind": "vote",
                "vote_question": "Ship through TCP?",
                "vote_options": ["Yes", "No"]
            }),
        )
        .await
        .unwrap_or_else(|error| panic!("create TCP vote: {error}"));
    let vote_id = created.outcome.event.id;
    store
        .execute_message_with_turn(
            &principal,
            "40000000-0000-4000-8000-000000000002",
            "message.send",
            &json!({
                "kind": "vote_cast",
                "vote_id": vote_id,
                "vote_choice": "Yes"
            }),
        )
        .await
        .unwrap_or_else(|error| panic!("cast TCP vote: {error}"));
    let last_seq = store
        .snapshot("general", 0, 1)
        .await
        .unwrap_or_else(|error| panic!("read vote high water: {error}"))
        .last_seq;
    (vote_id, last_seq)
}

async fn assert_local_tcp_summary(
    base_url: &str,
    state: &agentsassemble_server::AppState,
    store: &agentsassemble_persistence::SqliteStore,
    vote_id: &str,
    durable_last_seq: i64,
) -> serde_json::Value {
    let mut local = connect(base_url, state, "general").await;
    assert_eq!(local.subscribe(0).await["op"], "subscribed");
    assert_eq!(local.receive_json().await["op"], "snapshot");
    local
        .send_json(&json!({
            "op": "command",
            "request_id": "local-vote-summary",
            "action": "room.vote.summary",
            "payload": {"vote_id": vote_id}
        }))
        .await;
    let summary = local.receive_json().await;
    assert_eq!(summary["op"], "ack");
    assert_eq!(summary["resolution"], "committed");
    assert_eq!(summary["result"]["own_choice"], "Yes");
    assert_eq!(summary["result"]["tallies"]["Yes"], 1);
    assert_eq!(summary["result"]["total_votes"], 1);

    local
        .send_json(&json!({
            "op": "command",
            "request_id": "malformed-vote-summary",
            "action": "room.vote.summary",
            "payload": {"vote_id": vote_id, "room_id": "other"}
        }))
        .await;
    let malformed = local.receive_json().await;
    assert_eq!(malformed["op"], "nack");
    assert_eq!(malformed["resolution"], "rejected");
    assert_eq!(malformed["error"]["code"], "bad_request");
    local
        .send_json(&json!({"op": "ping", "nonce": "strict-read-stays-open"}))
        .await;
    assert_eq!(
        local.receive_json().await["nonce"],
        "strict-read-stays-open"
    );
    assert_eq!(
        store
            .snapshot("general", 0, 1)
            .await
            .unwrap_or_else(|error| panic!("read high water after local summaries: {error}"))
            .last_seq,
        durable_last_seq
    );
    local.close().await;
    summary["result"]["tallies"].clone()
}

fn local_principal() -> AuthenticatedPrincipal {
    AuthenticatedPrincipal {
        principal_id: LOCAL_OPERATOR_USER_ID.to_owned(),
        participant_id: LOCAL_OPERATOR_PARTICIPANT_ID.to_owned(),
        display_name: "SeiNel".to_owned(),
        room_id: "general".to_owned(),
        client_kind: ClientKind::Browser,
        invite_scope: InviteScope::ReadWrite,
        is_operator: true,
        capabilities: CapabilitySet::local_operator(ClientKind::Browser, InviteScope::ReadWrite),
    }
}
