use agentsassemble_domain::InviteScope;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use reqwest::Client;
use serde_json::{Value, json};
use uuid::Uuid;

#[path = "support/human_invite.rs"]
mod human_invite;
#[path = "support/room_socket_peer.rs"]
mod room_socket_peer;

#[path = "support/attendee.rs"]
mod attendee;
use attendee::fixture;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[tokio::test]
async fn attendee_http_publishes_external_custody_without_host_provider_discovery() -> TestResult {
    let (store, invite) = fixture().await?;
    // The fixture deliberately supplies an empty host catalog. The provider belongs to the caller.
    let server = human_invite::start(store.clone()).await;
    let mut events = server.rooms().subscribe("general").await;
    let client = Client::new();
    let endpoint = format!("{}/api/room-attendee/join", server.base_url);
    let mut body = json!({"request_id":Uuid::new_v4(),"client_secret":URL_SAFE_NO_PAD.encode([7;32]),
        "provider":"opencode", "display_name":"External Codex"});
    for (provider, expected_code) in [
        ("opencode", "provider_mismatch"),
        ("freebuff", "unsupported_provider"),
        ("antigravity", "unsupported_provider"),
    ] {
        body["provider"] = json!(provider);
        let response = client
            .post(&endpoint)
            .bearer_auth(&invite.invite_bearer)
            .json(&body)
            .send()
            .await?;
        assert_eq!(response.status(), 403);
        let error: Value = response.json().await?;
        assert_eq!(error["error"]["code"], expected_code);
    }
    assert!(
        store
            .snapshot("general", 0, 200)
            .await?
            .agent_sessions
            .is_empty()
    );
    body["provider"] = json!("codex");
    let response = client
        .post(&endpoint)
        .bearer_auth(&invite.invite_bearer)
        .json(&body)
        .send()
        .await?;
    assert_eq!(response.status(), 200);
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    let joined: Value = response.json().await?;
    let event = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let event = events.recv().await?;
            if event.event_type == "agent_session_created" {
                return Ok::<_, tokio::sync::broadcast::error::RecvError>(event);
            }
        }
    })
    .await??;
    assert_eq!(event.actor.participant_id, joined["participant_id"]);
    assert_eq!(
        event.extra["agent_session"]["process_ownership"],
        "external"
    );
    assert_eq!(event.extra["agent_session"]["enabled"], false);
    assert_eq!(
        event.extra["agent_session"]["provider_session_active"],
        false
    );
    verify_retry_and_isolation(
        &client,
        &endpoint,
        &server.base_url,
        &invite.invite_bearer,
        &body,
        &joined,
    )
    .await?;
    assert_eq!(
        store
            .snapshot("general", 0, 200)
            .await?
            .agent_sessions
            .len(),
        1
    );
    server.stop().await;
    Ok(())
}

async fn verify_retry_and_isolation(
    client: &Client,
    endpoint: &str,
    base_url: &str,
    invite: &str,
    body: &Value,
    joined: &Value,
) -> TestResult {
    let retry: Value = client
        .post(endpoint)
        .bearer_auth(invite)
        .json(body)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(retry["deduplicated"], true);
    let same_credential = retry["session_bearer"] == joined["session_bearer"];
    assert!(same_credential);
    assert_eq!(retry["participant_id"], joined["participant_id"]);
    assert_eq!(retry["expires_at"], joined["expires_at"]);
    let bearer = joined["session_bearer"]
        .as_str()
        .ok_or("attendee session missing")?;
    assert_eq!(
        client
            .post(endpoint)
            .bearer_auth(bearer)
            .json(body)
            .send()
            .await?
            .status(),
        401
    );
    assert_eq!(
        client
            .get(format!("{base_url}/api/room-connector/read"))
            .bearer_auth(bearer)
            .send()
            .await?
            .status(),
        401
    );
    assert!(
        !client
            .post(format!("{base_url}/api/session-tickets/socket"))
            .bearer_auth(bearer)
            .send()
            .await?
            .status()
            .is_success()
    );
    let mut competing = body.clone();
    competing["client_secret"] = json!(URL_SAFE_NO_PAD.encode([8; 32]));
    assert_eq!(
        client
            .post(endpoint)
            .bearer_auth(invite)
            .json(&competing)
            .send()
            .await?
            .status(),
        403
    );
    Ok(())
}

#[tokio::test]
async fn attendee_room_queue_publishes_ready_and_results_without_host_execution() -> TestResult {
    use agentsassemble_server::{
        AttendeeOperation as Operation, AttendeeOperationResult as ResultKind,
    };
    use sha2::{Digest as _, Sha256};
    let (store, invite) = fixture().await?;
    let human_invite = human_invite::persist_invite(
        &store,
        InviteScope::ReadWrite,
        1,
        "turn-human",
        "Turn Human",
    )
    .await;
    let server = human_invite::start(store.clone()).await;
    let mut events = server.rooms().subscribe("general").await;
    let admitted = server
        .rooms()
        .admit_attendee(agentsassemble_persistence::AttendeeAdmissionRequest {
            invite_fingerprint: &Sha256::digest(invite.invite_bearer.as_bytes()).into(),
            client_fingerprint: &[8; 32],
            request_id: Uuid::new_v4(),
            provider_kind: "codex",
            display_name: "External Codex",
        })
        .await?;
    let ResultKind::Connected(connection) = server
        .rooms()
        .execute_attendee(Operation::Connect {
            session: admitted.authorization,
            connection_id: Uuid::new_v4(),
        })
        .await?
    else {
        return Err("connection response mismatch".into());
    };
    let ready = serde_json::from_value(json!({
        "runtime_handle_id":"external-runtime", "runtime_owner_id":"external-owner", "runtime_lease_token":"external-lease",
        "provider_session_id":"external-session", "model":"contract-model", "reasoning_effort":"", "service_tier":"", "variant":"",
        "execution_harness":"builtin", "permission_mode":"meeting_read_only", "max_output_tokens":0, "retained_interrupt":true,
    }))?;
    server
        .rooms()
        .execute_attendee(Operation::Ready {
            connection: connection.clone(),
            report: Box::new(ready),
        })
        .await?;
    let client = Client::new();
    let human = human_invite::join(
        &client,
        &server.base_url,
        human_invite.invite_token(),
        &format!("aad1_{}", URL_SAFE_NO_PAD.encode([0xB7; 32])),
        &Uuid::new_v4().to_string(),
        "Turn Human",
        "",
    )
    .await;
    let mut socket = human_invite::open_session_socket(
        &client,
        &server.base_url,
        human_invite::canonical_session_token(&human),
    )
    .await;
    send_human_input(&mut socket).await?;
    let turn = store
        .deliver_attendee_turn(&connection, chrono::Utc::now())
        .await?
        .ok_or("external assignment missing")?;
    assert!(!turn.resume);
    assert!(turn.input.room_view.contains("Please reply externally"));
    let start = turn.authority;
    store
        .record_attendee_turn_started(
            &connection,
            &start,
            "queue-provider-turn",
            chrono::Utc::now(),
        )
        .await?;
    publish_external_result(server.rooms(), &mut events, &connection, start).await?;
    server
        .rooms()
        .execute_attendee(Operation::Disconnect { connection })
        .await?;
    let public = store
        .snapshot("general", 0, 200)
        .await?
        .agent_sessions
        .pop()
        .ok_or("external session missing")?;
    assert_eq!(
        public.runtime_status,
        agentsassemble_domain::AgentRuntimeStatus::Disconnected
    );
    assert!(public.provider_session_active);
    socket.close().await;
    server.stop().await;
    Ok(())
}

async fn publish_external_result(
    rooms: &agentsassemble_server::RoomRuntime,
    events: &mut tokio::sync::broadcast::Receiver<agentsassemble_domain::RoomEvent>,
    connection: &agentsassemble_persistence::AttendeeConnectionAuthorization,
    start: agentsassemble_persistence::ProviderTurnStartAuthority,
) -> TestResult {
    use agentsassemble_server::{
        AttendeeOperation as Operation, AttendeeOperationResult as ResultKind,
    };
    let report = agentsassemble_persistence::AttendeeTurnReport {
        request_id: Uuid::new_v4(),
        turn_id: start.turn_id,
        turn_generation: start.turn_generation,
        execution_id: start.execution_id,
        start_dispatch_nonce: start.start_dispatch_nonce,
        runtime_handle_id: start.runtime_handle_id,
        runtime_owner_id: start.runtime_owner_id,
        runtime_lease_token: start.runtime_lease_token,
        provider_turn_id: "queue-provider-turn".to_owned(),
        provider_session_id: None,
        outcome: agentsassemble_persistence::AttendeeTurnOutcome::Message {
            content: "External queue result".to_owned(),
            target_agent_id: String::new(),
        },
    };
    let result = rooms
        .execute_attendee(Operation::Report {
            connection: connection.clone(),
            report: Box::new(report.clone()),
        })
        .await?;
    let ResultKind::Reported {
        event_id,
        deduplicated,
        ..
    } = result
    else {
        return Err("report response mismatch".into());
    };
    assert!(!deduplicated);
    let published = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let event = events.recv().await?;
            if event.id == event_id {
                return Ok::<_, tokio::sync::broadcast::error::RecvError>(event);
            }
        }
    })
    .await??;
    assert_eq!(published.content.as_deref(), Some("External queue result"));
    let ResultKind::Reported { deduplicated, .. } = rooms
        .execute_attendee(Operation::Report {
            connection: connection.clone(),
            report: Box::new(report),
        })
        .await?
    else {
        return Err("retry response mismatch".into());
    };
    assert!(deduplicated);
    Ok(())
}

async fn send_human_input(
    socket: &mut room_socket_peer::RoomSocketPeer<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
) -> TestResult {
    socket.send_json(&json!({"op":"command", "request_id":"attendee-turn-input", "action":"message.send", "payload":{"content":"Please reply externally"}})).await;
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let frame = socket.receive_json().await;
            if frame["op"] == "ack" {
                break;
            }
            assert_ne!(frame["op"], "error");
        }
    })
    .await?;
    Ok(())
}
