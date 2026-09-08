use agentsassemble_domain::{
    InviteScope, ProviderCatalog, ProviderRequest, ProviderRequestKind, ProviderRequestPrompt,
    ProviderRequestQuestion, ProviderRequestResolution,
};
use agentsassemble_persistence::{
    AttendeeAdmissionRequest, AttendeeConnectionAuthorization, AttendeeRuntimeReady,
    CompanionInviteRequest, HumanAdmissionDecision, HumanAdmissionInput,
    HumanInviteCredentialEvidence, OpenProviderRequest, PreparedHumanAdmission,
    RoomSessionAuthorization, SqliteStore,
};
use agentsassemble_provider::ProviderCatalogService;
use agentsassemble_server::{AttendeeOperation, RoomRuntime};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, time::Duration};
use uuid::Uuid;

#[path = "support/human_invite.rs"]
mod human_invite;
#[path = "support/local_socket.rs"]
mod local_socket;
#[path = "support/room_socket_peer.rs"]
mod room_socket_peer;

#[path = "provider_request_broker/socket.rs"]
mod request_socket;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[tokio::test]
async fn live_secret_response_has_one_recipient_and_requires_native_ack() -> TestResult {
    let (store, rooms, human, connection, mut request) = fixture().await?;
    let id = request.request.provider_request_id;
    let opened = rooms
        .open_attendee_request(connection.clone(), request.clone())
        .await?;
    let mut native = opened.exchange.ok_or("exchange missing")?;
    let retry = rooms
        .open_attendee_request(connection.clone(), request.clone())
        .await?;
    assert!(retry.exchange.is_none() && retry.commit.deduplicated);
    assert_eq!(opened.commit.event.id, retry.commit.event.id);
    let answer = ProviderRequestResolution::Answers {
        answers: BTreeMap::from([("secret".to_owned(), vec!["live-only-value".to_owned()])]),
    };
    let (first, retry) = tokio::join!(
        rooms.resolve_live_provider_request(human.clone(), id, answer.clone()),
        rooms.resolve_live_provider_request(human.clone(), id, answer.clone())
    );
    assert_eq!(first?.event.id, retry?.event.id);
    assert!(native.receive().await? == answer);
    assert_eq!(
        store.pending_provider_request_ids("general").await?,
        vec![id]
    );
    tokio::time::timeout(Duration::from_secs(2), native.complete(true)).await??;
    assert!(
        store
            .pending_provider_request_ids("general")
            .await?
            .is_empty()
    );
    let events = store.snapshot("general", 0, 200).await?.events;
    assert!(!serde_json::to_string(&events)?.contains("live-only-value"));
    assert_eq!(
        events
            .iter()
            .filter(|event| event.event_type == "provider_request_closed"
                && event.extra["state"] == "resolved")
            .count(),
        1
    );

    request.request.provider_request_id = Uuid::new_v4();
    let opened = rooms
        .open_attendee_request(connection.clone(), request.clone())
        .await?;
    let mut native = opened.exchange.ok_or("replacement exchange missing")?;
    let mut events = rooms.subscribe("general").await;
    rooms
        .execute_attendee(AttendeeOperation::Disconnect { connection })
        .await?;
    assert!(
        tokio::time::timeout(Duration::from_secs(2), native.receive())
            .await?
            .is_err()
    );
    let closed = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let event = events.recv().await?;
            if event.event_type == "provider_request_closed" {
                break Ok::<_, tokio::sync::broadcast::error::RecvError>(event);
            }
        }
    })
    .await??;
    assert_eq!(closed.extra["state"], "cancelled");
    assert!(
        rooms
            .resolve_live_provider_request(human, request.request.provider_request_id, answer)
            .await
            .is_err()
    );
    rooms.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn dropped_native_wait_cancels_without_periodic_work() -> TestResult {
    let (store, rooms, _, connection, request) = fixture().await?;
    let mut events = rooms.subscribe("general").await;
    let opened = rooms.open_attendee_request(connection, request).await?;
    drop(opened.exchange);
    let closed = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let event = events.recv().await?;
            if event.event_type == "provider_request_closed" {
                break Ok::<_, tokio::sync::broadcast::error::RecvError>(event);
            }
        }
    })
    .await??;
    assert_eq!(closed.extra["state"], "cancelled");
    assert!(
        store
            .pending_provider_request_ids("general")
            .await?
            .is_empty()
    );
    rooms.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn stored_deadline_expires_the_live_recipient() -> TestResult {
    let (store, rooms, _, connection, request) = fixture().await?;
    let mut events = rooms.subscribe("general").await;
    let opened = rooms.open_attendee_request(connection, request).await?;
    let mut native = opened.exchange.ok_or("exchange missing")?;
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(16)).await;
    assert!(native.receive().await.is_err());
    let closed = loop {
        let event = events.recv().await?;
        if event.event_type == "provider_request_closed" {
            break event;
        }
    };
    assert_eq!(closed.extra["state"], "expired");
    assert!(
        store
            .pending_provider_request_ids("general")
            .await?
            .is_empty()
    );
    tokio::time::resume();
    rooms.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn authenticated_socket_routes_owner_answers_outside_generic_receipts() -> TestResult {
    let (store, human, bearer) = admitted_human().await?;
    let server = human_invite::start(store.clone()).await;
    let (connection, request, _) = assigned_request(&store, &human).await?;
    let id = request.request.provider_request_id;
    let opened = server
        .rooms()
        .open_attendee_request(connection, request)
        .await?;
    let mut native = opened.exchange.ok_or("exchange missing")?;
    let mut manager = local_socket::connect(&server.base_url, server.state(), "general").await;
    manager.subscribe(0).await;
    let command = serde_json::json!({"op":"command", "request_id":id, "action":"provider.request.resolve", "payload":{"response_kind":"answers", "answers":{"secret":["wire-secret-value"]}}});
    manager.send_json(&command).await;
    let rejected = receive_result(&mut manager, "nack").await;
    assert_eq!(rejected["error"]["code"], "permission_denied");
    let client = reqwest::Client::new();
    let mut owner = human_invite::open_session_socket(&client, &server.base_url, &bearer).await;
    owner.send_json(&command).await;
    let first = receive_result(&mut owner, "ack").await;
    assert!(first.get("deduplicated").is_none());
    assert_eq!(first["request_id"], id.to_string());
    owner.send_json(&command).await;
    let retry = receive_result(&mut owner, "ack").await;
    assert_eq!(retry["deduplicated"], true);
    assert_eq!(first["result"], retry["result"]);
    assert!(!first.to_string().contains("wire-secret-value"));
    let answer = native.receive().await?;
    assert!(
        matches!(answer, ProviderRequestResolution::Answers { answers } if answers["secret"] == ["wire-secret-value"])
    );
    native.complete(true).await?;
    owner.close().await;
    manager.close().await;
    server.stop().await;
    Ok(())
}

async fn receive_result(
    peer: &mut room_socket_peer::RoomSocketPeer<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    expected: &str,
) -> serde_json::Value {
    loop {
        let frame = peer.receive_json_with_timeout(Duration::from_secs(2)).await;
        if frame["op"] == "ack" || frame["op"] == "nack" {
            assert_eq!(frame["op"], expected);
            return frame;
        }
    }
}

async fn fixture() -> Result<
    (
        SqliteStore,
        RoomRuntime,
        RoomSessionAuthorization,
        AttendeeConnectionAuthorization,
        OpenProviderRequest,
    ),
    Box<dyn std::error::Error>,
> {
    let (store, human, _) = admitted_human().await?;
    let (connection, request, _) = assigned_request(&store, &human).await?;
    let rooms = RoomRuntime::new(
        store.clone(),
        ProviderCatalogService::fixed(ProviderCatalog::default()),
    );
    Ok((
        store,
        rooms,
        RoomSessionAuthorization::Human(human),
        connection,
        request,
    ))
}

async fn assigned_request(
    store: &SqliteStore,
    human: &agentsassemble_persistence::HumanSessionAuthorization,
) -> Result<
    (AttendeeConnectionAuthorization, OpenProviderRequest, String),
    Box<dyn std::error::Error>,
> {
    let now = chrono::Utc::now();
    let invite = store
        .create_companion_attendee_invite(
            human,
            CompanionInviteRequest {
                request_id: Uuid::new_v4(),
                provider_kind: "codex_live_session",
                display_name: "Request Companion",
            },
            now,
        )
        .await?;
    let admitted = store
        .admit_attendee(
            AttendeeAdmissionRequest {
                invite_fingerprint: &Sha256::digest(invite.invite_bearer.as_bytes()).into(),
                client_fingerprint: &Sha256::digest(Uuid::new_v4().as_bytes()).into(),
                request_id: Uuid::new_v4(),
                provider_kind: "codex_live_session",
                display_name: "Request Companion",
            },
            now,
        )
        .await?;
    let connection = store
        .claim_attendee_connection(&admitted.authorization, Uuid::new_v4(), now)
        .await?
        .authorization;
    store
        .record_attendee_ready(&connection, &ready_report(), now)
        .await?;
    store
        .execute_authorized_message_with_turn(
            agentsassemble_persistence::RoomMutationAuthority::HumanSession(human),
            &Uuid::new_v4().to_string(),
            "message.send",
            &serde_json::json!({"content":"Request input"}),
        )
        .await?;
    let turn = store
        .deliver_attendee_turn(&connection, now)
        .await?
        .ok_or("turn missing")?;
    store
        .record_attendee_turn_started(&connection, &turn.authority, "request-native-turn", now)
        .await?;
    let request = OpenProviderRequest {
        turn_generation: turn.authority.turn_generation,
        execution_id: turn.authority.execution_id,
        request: ProviderRequest {
            provider_request_id: Uuid::new_v4(),
            request_kind: ProviderRequestKind::UserInput,
            title: "Input".to_owned(),
            description: String::new(),
            timeout_seconds: 15,
            prompt: ProviderRequestPrompt::Answers {
                questions: vec![ProviderRequestQuestion {
                    id: "secret".to_owned(),
                    header: String::new(),
                    question: "Enter value".to_owned(),
                    options: Vec::new(),
                    multiple: false,
                    is_other: true,
                    is_secret: true,
                }],
            },
        },
    };
    Ok((connection, request, admitted.session_bearer))
}

async fn admitted_human() -> Result<
    (
        SqliteStore,
        agentsassemble_persistence::HumanSessionAuthorization,
        String,
    ),
    Box<dyn std::error::Error>,
> {
    let (store, invite) = human_invite::fixture(InviteScope::ReadWrite).await;
    let prepared = PreparedHumanAdmission::prepare(
        HumanInviteCredentialEvidence::JoinCode {
            fingerprint: *invite.join_code_fingerprint(),
        },
        Sha256::digest(Uuid::new_v4().as_bytes()).into(),
        &HumanAdmissionInput {
            request_id: Uuid::new_v4().to_string(),
            meeting_id_assertion: "general".to_owned(),
            display_name: "Request Owner".to_owned(),
            participant_type: "human".to_owned(),
            owner_display_name: String::new(),
            client_id: "request-broker-fixture".to_owned(),
            avatar_image_url: String::new(),
        },
    )?;
    let HumanAdmissionDecision::Admitted(admitted) =
        store.admit_human(&prepared, chrono::Utc::now()).await?
    else {
        return Err("human not admitted".into());
    };
    let human = store
        .authorize_human_session(&Sha256::digest(admitted.session_bearer().as_bytes()).into())
        .await?;
    Ok((store, human, admitted.session_bearer().to_owned()))
}

fn ready_report() -> AttendeeRuntimeReady {
    AttendeeRuntimeReady {
        retained_interrupt: true,
        runtime_handle_id: "request-runtime".to_owned(),
        runtime_owner_id: "request-owner".to_owned(),
        runtime_lease_token: "request-lease".to_owned(),
        provider_session_id: "request-provider".to_owned(),
        model: "fixture".to_owned(),
        reasoning_effort: "high".to_owned(),
        service_tier: String::new(),
        variant: String::new(),
        execution_harness: "builtin".to_owned(),
        permission_mode: "meeting_read_only".to_owned(),
        max_output_tokens: 0,
    }
}
