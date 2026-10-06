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
    let result = if agent {
        agent_avatar::upload(State(state.clone()), Path("missing".into()), request).await
    } else {
        upload_attachment(State(state.clone()), request).await
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
    let profile = upload_attachment(State(state.clone()), Request::new(body::Body::empty())).await;
    let agent = agent_avatar::upload(
        State(state.clone()),
        Path("missing".into()),
        Request::new(body::Body::empty()),
    )
    .await;
    assert_eq!(profile.into_response().status(), StatusCode::UNAUTHORIZED);
    assert_eq!(agent.into_response().status(), StatusCode::UNAUTHORIZED);
}
