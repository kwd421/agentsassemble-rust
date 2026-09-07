use std::time::Duration;

use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::{
    SessionCreationAuthority, guarded_session_creation, isolated_config_root, isolated_environment,
    observe_startup, server_arguments, server_password,
};
use crate::{loopback_http::LoopbackHttp, opencode_protocol::http_driver_error};

#[test]
fn server_launch_disables_external_and_project_configuration() {
    let arguments = server_arguments(43123);
    assert!(arguments.iter().any(|argument| argument == "--pure"));

    let root = tempfile::tempdir()
        .unwrap_or_else(|error| panic!("create OpenCode config fixture: {error}"));
    let password = server_password();
    let environment = isolated_environment(root.path(), &password)
        .unwrap_or_else(|_| panic!("build OpenCode environment"));
    let value = |name: &str| {
        environment
            .iter()
            .find_map(|(key, value)| (key == name).then_some(value.as_str()))
    };
    assert_eq!(value("OPENCODE_DISABLE_PROJECT_CONFIG"), Some("1"));
    assert_eq!(value("OPENCODE_PURE"), Some("1"));
    assert_eq!(value("OPENCODE_DISABLE_EXTERNAL_SKILLS"), Some("1"));
    assert_eq!(value("XDG_CONFIG_HOME"), root.path().to_str());
    assert_eq!(value("OPENCODE_SERVER_USERNAME"), Some("agentsassemble"));
    assert_eq!(value("OPENCODE_SERVER_PASSWORD"), Some(password.as_str()));
    assert_eq!(password.len(), 64);
    assert!(password.bytes().all(|byte| byte.is_ascii_hexdigit()));
}

#[test]
fn isolated_config_root_is_private_on_the_running_platform() {
    let root = isolated_config_root()
        .unwrap_or_else(|error| panic!("create isolated OpenCode config root: {error:?}"));
    assert!(root.path().is_dir());
}

#[tokio::test]
async fn startup_requires_the_exact_bound_child_endpoint() {
    let expected = "opencode server listening on http://127.0.0.1:43123";
    let (mut accepted_writer, accepted_reader) = tokio::io::duplex(1024);
    let (accepted_task, accepted) = observe_startup(accepted_reader, expected.to_owned());
    accepted_writer
        .write_all(format!("{expected}\n").as_bytes())
        .await
        .unwrap_or_else(|error| panic!("write accepted line: {error}"));
    drop(accepted_writer);
    assert_eq!(accepted.await, Ok(true));
    accepted_task
        .await
        .unwrap_or_else(|error| panic!("join accepted output: {error}"));

    let (mut rejected_writer, rejected_reader) = tokio::io::duplex(1024);
    let (rejected_task, rejected) = observe_startup(rejected_reader, expected.to_owned());
    rejected_writer
        .write_all(b"opencode server listening on http://127.0.0.1:43124\n")
        .await
        .unwrap_or_else(|error| panic!("write rejected line: {error}"));
    drop(rejected_writer);
    assert_eq!(rejected.await, Ok(false));
    rejected_task
        .await
        .unwrap_or_else(|error| panic!("join rejected output: {error}"));
}

#[tokio::test]
async fn unconfirmed_session_creation_never_polls_a_second_provider_effect() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap_or_else(|error| panic!("bind OpenCode fixture: {error}"));
    let endpoint = format!(
        "http://127.0.0.1:{}/",
        listener
            .local_addr()
            .unwrap_or_else(|error| panic!("read fixture address: {error}"))
            .port()
    );
    let observed = tokio::spawn(async move {
        let (mut first, _) = listener
            .accept()
            .await
            .unwrap_or_else(|error| panic!("accept first session request: {error}"));
        let mut first_bytes = vec![0_u8; 16 * 1024];
        let first_length = first
            .read(&mut first_bytes)
            .await
            .unwrap_or_else(|error| panic!("read first session request: {error}"));
        first_bytes.truncate(first_length);
        drop(first);
        let (mut second, _) = listener
            .accept()
            .await
            .unwrap_or_else(|error| panic!("accept guarded retry connection: {error}"));
        let mut second_bytes = Vec::new();
        second
            .read_to_end(&mut second_bytes)
            .await
            .unwrap_or_else(|error| panic!("read guarded retry connection: {error}"));
        (first_bytes, second_bytes)
    });
    let directory = tempfile::tempdir()
        .unwrap_or_else(|error| panic!("create OpenCode request fixture: {error}"));
    let http = LoopbackHttp::new(
        &endpoint,
        directory.path(),
        "agentsassemble",
        &"x".repeat(64),
    )
    .unwrap_or_else(|error| panic!("create OpenCode loopback client: {error}"));
    let mut authority = SessionCreationAuthority::default();
    let first_connection = http
        .verify_peer(
            http.connect()
                .await
                .unwrap_or_else(|error| panic!("connect first request: {error}")),
            true,
        )
        .unwrap_or_else(|error| panic!("verify first request: {error}"));
    let first = guarded_session_creation(&mut authority, async move {
        first_connection
            .post_json(
                "/session",
                &json!({"title": "exact"}),
                Duration::from_secs(1),
            )
            .await
            .map(|_| ())
            .map_err(http_driver_error)
    })
    .await;
    let Err(first_error) = first else {
        panic!("unconfirmed provider request must fail");
    };
    assert_eq!(first_error.code, "provider_transport_failed");

    let second_connection = http
        .verify_peer(
            http.connect()
                .await
                .unwrap_or_else(|error| panic!("connect guarded retry: {error}")),
            true,
        )
        .unwrap_or_else(|error| panic!("verify guarded retry: {error}"));
    let second = guarded_session_creation(&mut authority, async move {
        second_connection
            .post_json(
                "/session",
                &json!({"title": "duplicate"}),
                Duration::from_secs(1),
            )
            .await
            .map(|_| ())
            .map_err(http_driver_error)
    })
    .await;
    let Err(second_error) = second else {
        panic!("unconfirmed provider request must block retry");
    };
    assert_eq!(second_error.code, "provider_session_unconfirmed");
    let (first_bytes, second_bytes) = observed
        .await
        .unwrap_or_else(|error| panic!("join OpenCode request fixture: {error}"));
    assert!(String::from_utf8_lossy(&first_bytes).starts_with("POST /session?"));
    assert!(second_bytes.is_empty(), "guarded retry sent provider bytes");
}

#[tokio::test]
async fn tool_only_completion_preserves_portal_validation_and_native_authority() {
    use crate::{
        driver::{ProviderRoomObservation, ProviderTurnRequest},
        loopback_http::JsonResponse,
        opencode_sse::OpenCodeTurnEvents,
        test_support::durable_session,
    };
    let session = durable_session(
        "room",
        "agent",
        "Agent",
        "opencode",
        "opencode/model",
        "http_sse",
    );
    let mut request = ProviderTurnRequest {
        turn_id: "turn".to_owned(),
        turn_generation: 1,
        execution_id: "00000000-0000-4000-8000-000000000099".to_owned(),
        input: "Reply".to_owned(),
        room_observation: None,
    };
    let response = JsonResponse {
        status: hyper::StatusCode::OK,
        value: json!({"info": {"id": "assistant", "parentID": "request", "role": "assistant",
            "providerID": "opencode", "modelID": "model"}, "parts": []}),
    };
    let mut events = OpenCodeTurnEvents {
        request_message: "request".to_owned(),
        assistant_message: "assistant".to_owned(),
        observed_model: "opencode/model".to_owned(),
    };
    assert!(
        super::OpenCodeDriver::completed_from_response(
            &session,
            &request,
            "native-session",
            &response,
            &events
        )
        .is_err()
    );
    request.room_observation = Some(ProviderRoomObservation {
        session_id: "agent".to_owned(),
        input_up_to_seq: 1,
        view: "#1 Human: reply".to_owned(),
        attachment_ids: vec![],
        attachment_ingress: None,
        allowed_agent_ids: vec![],
        tabletop_tools: false,
        room_tool_ingress: None,
    });
    let completed = super::OpenCodeDriver::completed_from_response(
        &session,
        &request,
        "native-session",
        &response,
        &events,
    )
    .unwrap_or_else(|error| panic!("allow room publication validation: {error}"));
    assert_eq!(completed.provider_turn_id, "assistant");
    assert_eq!(
        completed.provider_session_id.as_deref(),
        Some("native-session")
    );
    events.observed_model = "opencode/wrong".to_owned();
    assert!(
        super::OpenCodeDriver::completed_from_response(
            &session,
            &request,
            "native-session",
            &response,
            &events
        )
        .is_err()
    );
    assert_tool_only_portal_outcome(&request, false).await;
    assert_tool_only_portal_outcome(&request, true).await;
}

async fn assert_tool_only_portal_outcome(request: &crate::ProviderTurnRequest, decline: bool) {
    use crate::room_portal::{ProviderTurnOutcome, RoomPortal, RoomPortalError};
    use rmcp::{
        ServiceExt,
        model::CallToolRequestParams,
        transport::{
            StreamableHttpClientTransport,
            streamable_http_client::StreamableHttpClientTransportConfig,
        },
    };
    let mut portal = RoomPortal::create()
        .await
        .unwrap_or_else(|error| panic!("create portal: {error}"));
    portal
        .begin_turn(request)
        .unwrap_or_else(|error| panic!("begin portal: {error}"));
    assert!(matches!(
        portal.finish_turn(request),
        Err(RoomPortalError::ReceiptMissing)
    ));
    let client = ()
        .serve(StreamableHttpClientTransport::from_config(
            StreamableHttpClientTransportConfig::with_uri(portal.endpoint())
                .auth_header(portal.bearer_token()),
        ))
        .await
        .unwrap_or_else(|error| panic!("connect MCP: {error}"));
    let terminal_tool = if decline {
        ("decline_to_speak", json!({"reason_code": "duplicate"}))
    } else {
        (
            "publish_message",
            json!({"content": "Published reply", "next_agent_id": ""}),
        )
    };
    for (tool, arguments) in [("read_discussion", json!({})), terminal_tool] {
        let result = client
            .call_tool(
                CallToolRequestParams::new(tool)
                    .with_arguments(arguments.as_object().cloned().unwrap_or_default()),
            )
            .await
            .unwrap_or_else(|error| panic!("call room tool: {error}"));
        assert_ne!(result.is_error, Some(true));
        if tool == "read_discussion" {
            assert!(matches!(
                portal.finish_turn(request),
                Err(RoomPortalError::OutcomeMissing)
            ));
        }
    }
    assert_eq!(
        portal
            .finish_turn(request)
            .unwrap_or_else(|error| panic!("finish publication: {error}")),
        if decline {
            ProviderTurnOutcome::Declined {
                reason_code: "duplicate".to_owned(),
            }
        } else {
            ProviderTurnOutcome::Message {
                content: "Published reply".to_owned(),
                target_agent_id: String::new(),
            }
        }
    );
    client
        .cancel()
        .await
        .unwrap_or_else(|error| panic!("close MCP: {error}"));
    portal
        .shutdown()
        .await
        .unwrap_or_else(|error| panic!("close portal: {error}"));
}
