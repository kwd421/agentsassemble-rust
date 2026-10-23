use super::*;
use crate::{TicketStore, ticket_tests::HumanSessionFixture};
use agentsassemble_domain::{LOCAL_OPERATOR_PARTICIPANT_ID, ProviderCatalog};
use agentsassemble_provider::ProviderCatalogService;

async fn fixture() -> AppState {
    let fixture = HumanSessionFixture::new(0).await;
    AppState::local(
        fixture.store().clone(),
        TicketStore::new(std::time::Duration::from_secs(30), 32),
        ProviderCatalogService::fixed(ProviderCatalog::default()),
    )
    .await
    .unwrap_or_else(|error| panic!("avatar state: {error:?}"))
}

async fn read(state: &AppState, agent: bool) -> Response {
    let request = Request::new(body::Body::empty());
    let result = if agent {
        agent_avatar::read(State(state.clone()), Path("missing".into()), request).await
    } else {
        read_attachment(
            State(state.clone()),
            Path("missing".into()),
            Query(std::collections::HashMap::default()),
            RawQuery(None),
            request,
        )
        .await
    };
    result.unwrap_or_else(IntoResponse::into_response)
}

async fn assert_busy(response: Response) {
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    let bytes = body::to_bytes(response.into_body(), 1024)
        .await
        .unwrap_or_else(|error| panic!("error body: {error:?}"));
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&bytes)
            .unwrap_or_else(|error| panic!("error JSON: {error:?}")),
        json!({"error": "잠시 후 다시 시도해 주세요.", "code": "too_many_requests"})
    );
}

#[tokio::test]
async fn shared_read_limit_retains_permit_with_response_bytes_and_recovers() {
    let state = fixture().await;
    let mut responses = Vec::new();
    for _ in 0..6 {
        let permit = avatar_admission::acquire(&state.avatar_reads)
            .unwrap_or_else(|error| panic!("six reads: {error:?}"));
        responses.push(
            avatar_admission::response("avatar.png", "image/png", vec![1], true, permit)
                .unwrap_or_else(|error| panic!("avatar response: {error:?}")),
        );
    }
    for agent in [false, true] {
        assert_busy(read(&state, agent).await).await;
    }
    let bytes = body::to_bytes(
        responses
            .pop()
            .unwrap_or_else(|| panic!("response"))
            .into_body(),
        10,
    )
    .await
    .unwrap_or_else(|error| panic!("transport bytes: {error:?}"));
    let retained = bytes.clone();
    drop(bytes);
    for agent in [false, true] {
        assert_busy(read(&state, agent).await).await;
    }
    drop(retained);
    for agent in [false, true] {
        assert_eq!(read(&state, agent).await.status(), StatusCode::NOT_FOUND);
    }
    // Failed lookups return their permit, and a new successful response can use it.
    let permit = avatar_admission::acquire(&state.avatar_reads)
        .unwrap_or_else(|error| panic!("released slot: {error:?}"));
    let response = avatar_admission::response("avatar.png", "image/png", vec![1], true, permit)
        .unwrap_or_else(|error| panic!("recovered response: {error:?}"));
    assert_eq!(response.status(), StatusCode::OK);
    drop(response);
    drop(responses);
    assert!(state.avatar_reads.try_acquire_many(6).is_ok());
}

async fn upload(state: &AppState, agent: bool, content: body::Body) -> Response {
    let ticket = if agent {
        let manager = state
            .store
            .authorize_local_room_manager(
                "general",
                LOCAL_OPERATOR_USER_ID,
                LOCAL_OPERATOR_PARTICIPANT_ID,
            )
            .await
            .unwrap_or_else(|error| panic!("manager: {error:?}"));
        state
            .tickets
            .issue_agent_avatar_upload(manager, "missing".into())
            .await
    } else {
        state
            .tickets
            .issue_server_operator(LOCAL_OPERATOR_USER_ID.into())
            .await
    }
    .unwrap_or_else(|error| panic!("upload ticket: {error:?}"));
    let request = Request::builder()
        .header(header::AUTHORIZATION, format!("Bearer {}", ticket.ticket))
        .body(content)
        .unwrap_or_else(|error| panic!("request: {error:?}"));
    let policy = axum::Extension(crate::http_body_policy::Policy::new());
    let result = if agent {
        agent_avatar::upload(
            State(state.clone()),
            Path("missing".into()),
            policy,
            request,
        )
        .await
    } else {
        upload_attachment(State(state.clone()), policy, request).await
    };
    result.into_response()
}

#[tokio::test]
async fn shared_upload_limit_rejects_before_polling_body_and_recovers() {
    let state = fixture().await;
    let first = avatar_admission::acquire(&state.avatar_uploads)
        .unwrap_or_else(|error| panic!("first upload: {error:?}"));
    let second = avatar_admission::acquire(&state.avatar_uploads)
        .unwrap_or_else(|error| panic!("second upload: {error:?}"));
    for agent in [false, true] {
        let stream = futures_util::stream::poll_fn(
            |_| -> std::task::Poll<Option<Result<body::Bytes, std::io::Error>>> {
                panic!("saturated uploads must not poll the body")
            },
        );
        assert_busy(upload(&state, agent, body::Body::from_stream(stream)).await).await;
    }
    drop(first);
    for agent in [false, true] {
        // A malformed body reaches JSON parsing again after admission is released.
        assert_eq!(
            upload(&state, agent, body::Body::from("{")).await.status(),
            StatusCode::BAD_REQUEST
        );
    }
    let payload = json!({"purpose": "profile_avatar", "filename": "avatar.png", "content_type": "image/png",
        "data_base64": "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGMQ0bD5DwACRAF4aig0hQAAAABJRU5ErkJggg=="});
    assert_eq!(
        upload(&state, false, body::Body::from(payload.to_string()))
            .await
            .status(),
        StatusCode::OK
    );
    drop(second);
    assert!(state.avatar_uploads.try_acquire_many(2).is_ok());
}

#[tokio::test]
async fn upload_authority_rejection_precedes_saturation() {
    let state = fixture().await;
    let _held = state
        .avatar_uploads
        .try_acquire_many(2)
        .unwrap_or_else(|error| panic!("saturate: {error:?}"));
    let policy = axum::Extension(crate::http_body_policy::Policy::new());
    let profile = upload_attachment(
        State(state.clone()),
        policy.clone(),
        Request::new(body::Body::empty()),
    )
    .await;
    let agent = agent_avatar::upload(
        State(state.clone()),
        Path("missing".into()),
        policy,
        Request::new(body::Body::empty()),
    )
    .await;
    assert_eq!(profile.into_response().status(), StatusCode::UNAUTHORIZED);
    assert_eq!(agent.into_response().status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn retained_body_saturation_is_retryable_before_the_authorized_route_reads() {
    let state = fixture().await;
    let empty = Request::new(body::Body::empty());
    let (_, first) = state
        .upload_bodies
        .reserve(
            Some(LOCAL_OPERATOR_USER_ID),
            &empty,
            MAX_BASE64_UPLOAD_BODY_BYTES,
        )
        .expect("first");
    let (_, second) = state
        .upload_bodies
        .reserve(
            Some(LOCAL_OPERATOR_USER_ID),
            &empty,
            MAX_BASE64_UPLOAD_BODY_BYTES,
        )
        .expect("second");
    let body = body::Body::from_stream(futures_util::stream::poll_fn(
        |_| -> std::task::Poll<Option<Result<body::Bytes, std::io::Error>>> {
            panic!("refused request must not read the body")
        },
    ));
    let response = upload(&state, false, body).await;
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    let encoded = body::to_bytes(response.into_body(), 1024)
        .await
        .expect("error body");
    let error: serde_json::Value = serde_json::from_slice(&encoded).expect("JSON");
    assert_eq!(error["code"], "upload_body_budget_exhausted");
    drop((first, second));
    // After release the same route reaches its ordinary JSON validation.
    assert_eq!(
        upload(&state, false, body::Body::from("invalid"))
            .await
            .status(),
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn large_download_responses_enforce_host_and_principal_budgets_until_last_clone_drops() {
    use agentsassemble_domain::{CapabilitySet, ClientKind, InviteScope, public_settings};
    use agentsassemble_persistence::RoomMutationAuthority::TrustedPrincipal;
    use futures_util::StreamExt;
    let fixture = HumanSessionFixture::new(1).await;
    let other = fixture.authorize(0).await;
    let state = AppState::local(
        fixture.store().clone(),
        TicketStore::new(std::time::Duration::from_secs(30), 32),
        ProviderCatalogService::fixed(ProviderCatalog::default()),
    )
    .await
    .unwrap();
    let principal = agentsassemble_domain::AuthenticatedPrincipal {
        principal_id: LOCAL_OPERATOR_USER_ID.into(),
        participant_id: LOCAL_OPERATOR_PARTICIPANT_ID.into(),
        room_id: "general".into(),
        display_name: "Host".into(),
        client_kind: ClientKind::Browser,
        invite_scope: InviteScope::ReadWrite,
        is_operator: true,
        capabilities: CapabilitySet::local_operator(ClientKind::Browser, InviteScope::ReadWrite),
    };
    let settings = state
        .store
        .snapshot("general", 0, 100)
        .await
        .unwrap()
        .settings;
    state.store.execute_room_settings_update(TrustedPrincipal(&principal), "large-policy", &json!({
        "expected_revision":public_settings(&settings).unwrap().settings_revision, "attachment_file_limit_bytes":52_428_800
    })).await.unwrap();
    let attachment = state
        .store
        .store_message_attachment(
            &principal,
            "large.bin",
            "application/octet-stream",
            vec![42; 52_428_800],
        )
        .await
        .unwrap();
    state
        .store
        .execute_message_with_turn(
            &principal,
            "large-file",
            "message.send",
            &json!({"content":"", "attachment_ids":[attachment.id]}),
        )
        .await
        .unwrap();
    let mut responses = Vec::new();
    for _ in 0..32 {
        let response = read_message(&state, &attachment.id, &principal).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CONTENT_LENGTH], "52428800");
        responses.push(response);
    }
    assert_download_busy(read_message(&state, &attachment.id, &principal).await).await;
    let other_response = read_message(&state, &attachment.id, other.principal()).await;
    assert_eq!(other_response.status(), StatusCode::OK);
    let mut body = responses.pop().unwrap().into_body().into_data_stream();
    let bytes = body.next().await.unwrap().unwrap();
    assert!(bytes.len() == 65536 && bytes.iter().all(|&byte| byte == 42));
    let clone = bytes.clone();
    drop((bytes, body));
    assert_download_busy(read_message(&state, &attachment.id, &principal).await).await;
    drop(clone);
    responses.push(read_message(&state, &attachment.id, &principal).await);
    assert_eq!(responses.last().unwrap().status(), StatusCode::OK);
    let host = state
        .downloads
        .reserve(None, (512 - 33) * 1024 * 1024 / 2)
        .unwrap();
    assert_download_busy(read_message(&state, &attachment.id, other.principal()).await).await;
    drop((host, responses, other_response));
    assert_eq!(state.downloads.available(), 512 * 1024 * 1024);
}

async fn read_message(
    state: &AppState,
    id: &str,
    principal: &agentsassemble_domain::AuthenticatedPrincipal,
) -> Response {
    let ticket = state
        .tickets
        .issue_bound_message_attachment_read(
            principal.room_id.clone(),
            principal.principal_id.clone(),
            principal.participant_id.clone(),
            id.into(),
        )
        .await
        .unwrap();
    let request = Request::builder()
        .header(header::AUTHORIZATION, format!("Bearer {}", ticket.ticket))
        .body(body::Body::empty())
        .unwrap();
    read_attachment(
        State(state.clone()),
        Path(id.into()),
        Query(std::collections::HashMap::default()),
        RawQuery(Some("download=1".into())),
        request,
    )
    .await
    .unwrap_or_else(IntoResponse::into_response)
}
async fn assert_download_busy(response: Response) {
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    let bytes = body::to_bytes(response.into_body(), 1024).await.unwrap();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["code"], "attachment_download_budget_exhausted");
}
