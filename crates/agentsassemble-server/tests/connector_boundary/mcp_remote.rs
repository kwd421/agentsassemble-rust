use super::{InviteScope, Uuid, fixture, human_invite, json, mcp::call};
use agentsassemble_server::connector_mcp::ConnectorMcp;
use rmcp::{
    RoleClient, ServiceExt,
    model::CallToolRequestParams,
    service::RunningService,
    transport::{
        StreamableHttpClientTransport,
        streamable_http_server::{
            StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
        },
    },
};
use serde_json::Value;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn remote_http_requires_private_custody_before_admission_and_retains_uncertain_retry()
-> Result<(), Box<dyn std::error::Error>> {
    let (store, manager) = fixture().await?;
    let server = human_invite::start(store.clone()).await;
    let relay =
        super::lossy_http::LossyHttpRelay::start(&server.base_url, &["/api/room-connector/join"])
            .await?;
    let connector = ConnectorMcp::new(Some(vec![relay.base_url.clone()]))?;
    let factory = connector.clone();
    let cancellation = CancellationToken::new();
    let service = StreamableHttpService::new(
        move || Ok(factory.clone()),
        Arc::<LocalSessionManager>::default(),
        StreamableHttpServerConfig::default()
            .with_legacy_session_mode(false)
            .with_json_response(true)
            .with_sse_keep_alive(None)
            .with_cancellation_token(cancellation.child_token()),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let endpoint = format!("http://{}/mcp", listener.local_addr()?);
    let stopping = cancellation.clone();
    let task = tokio::spawn(async move {
        axum::serve(listener, axum::Router::new().nest_service("/mcp", service))
            .with_graceful_shutdown(stopping.cancelled_owned())
            .await
    });
    let first = ().serve(StreamableHttpClientTransport::from_uri(endpoint.clone())).await?;
    let second = ().serve(StreamableHttpClientTransport::from_uri(endpoint)).await?;
    let tools = first.list_all_tools().await?;
    assert_eq!(tools.len(), 14);
    rejected(
        &first,
        "room_join",
        json!({"invite_url":"https://unapproved.example.test/join?token=test"}),
        "room_server_not_allowed",
    )
    .await;
    let invite = store
        .create_connector_invite(
            &manager,
            Uuid::new_v4(),
            InviteScope::ReadWrite,
            chrono::Utc::now(),
        )
        .await?;
    let invitation =
        json!({"invite_url":format!("{}/join?token={}",relay.base_url,invite.invite_bearer)});
    let joined = verify_custody(&store, &first, &second, invitation).await?;

    // The same remote endpoint still supports independent valid invitations.
    let invite = store
        .create_connector_invite(
            &manager,
            Uuid::new_v4(),
            InviteScope::ReadWrite,
            chrono::Utc::now(),
        )
        .await?;
    let mut another = json!({"invite_url":format!("{}/join?token={}",relay.base_url,invite.invite_bearer),"display_name":"Second conversation"});
    let prepared = call(&second, "room_join", another.clone()).await;
    another["connection_id"] = prepared["connection_id"].clone();
    let other_joined = call(&second, "room_join", another).await;
    assert_ne!(other_joined["participant_id"], joined["participant_id"]);
    for (client, connection) in [(&first, joined), (&second, other_joined)] {
        call(
            client,
            "room_leave",
            json!({"connection_id":connection["connection_id"]}),
        )
        .await;
    }
    first.cancel().await?;
    second.cancel().await?;
    connector.close();
    cancellation.cancel();
    task.await??;
    relay.stop().await?;
    server.stop().await;
    Ok(())
}

async fn verify_custody(
    store: &agentsassemble_persistence::SqliteStore,
    first: &RunningService<RoleClient, ()>,
    second: &RunningService<RoleClient, ()>,
    invitation: Value,
) -> Result<Value, Box<dyn std::error::Error>> {
    let prepared = call(first, "room_join", invitation.clone()).await;
    assert_eq!(prepared["status"], "connection_prepared");
    cancel_preparation(second, &invitation).await;
    let snapshot = store.snapshot("general", 0, 200).await?;
    assert!(
        snapshot
            .participants
            .iter()
            .all(|p| p.participant_type != "agent")
    );
    let mut confirmation = invitation.clone();
    confirmation["connection_id"] = prepared["connection_id"].clone();
    rejected(
        first,
        "room_join",
        confirmation.clone(),
        "invalid_connector_response",
    )
    .await;
    let snapshot = store.snapshot("general", 0, 200).await?;
    assert_eq!(
        snapshot
            .participants
            .iter()
            .filter(|p| p.participant_type == "agent")
            .count(),
        1
    );

    // A separate MCP conversation knows the consumed URL and the same default
    // display name, but none of the original private retry/connection credentials.
    let other = call(second, "room_join", invitation.clone()).await;
    assert_eq!(other["status"], "connection_prepared");
    assert_ne!(other["connection_id"], prepared["connection_id"]);
    let mut other_confirmation = invitation;
    other_confirmation["connection_id"] = other["connection_id"].clone();
    rejected(
        second,
        "room_join",
        other_confirmation,
        "invite_already_used",
    )
    .await;
    rejected(
        second,
        "room_read",
        json!({"connection_id":other["connection_id"]}),
        "invalid_connection_id",
    )
    .await;
    for arguments in [json!({}), json!({"connection_id":"unknown"})] {
        rejected(second, "room_read", arguments, "invalid_connection_id").await;
    }
    let mut changed = confirmation.clone();
    changed["display_name"] = json!("Changed identity");
    rejected(
        first,
        "room_join",
        changed,
        "connector_join_identity_conflict",
    )
    .await;
    let joined = call(first, "room_join", confirmation.clone()).await;
    assert_eq!(joined["status"], "joined");
    assert_eq!(joined["connection_id"], prepared["connection_id"]);
    let repeated = call(first, "room_join", confirmation).await;
    assert_eq!(repeated["participant_id"], joined["participant_id"]);
    call(
        first,
        "room_read",
        json!({"connection_id":joined["connection_id"]}),
    )
    .await;
    call(
        first,
        "room_say",
        json!({"connection_id":joined["connection_id"],"content":"Original owner retained"}),
    )
    .await;
    let snapshot = store.snapshot("general", 0, 200).await?;
    assert_eq!(
        snapshot
            .participants
            .iter()
            .filter(|p| p.participant_type == "agent")
            .count(),
        1
    );
    assert_eq!(
        snapshot
            .events
            .iter()
            .filter(|e| e.content.as_deref() == Some("Original owner retained"))
            .count(),
        1
    );

    Ok(joined)
}

async fn cancel_preparation(client: &RunningService<RoleClient, ()>, invitation: &Value) {
    let abandoned = call(client, "room_join", invitation.clone()).await;
    assert_eq!(
        call(
            client,
            "room_leave",
            json!({"connection_id":abandoned["connection_id"]})
        )
        .await["status"],
        "connection_cancelled"
    );
    let mut cancelled = invitation.clone();
    cancelled["connection_id"] = abandoned["connection_id"].clone();
    rejected(client, "room_join", cancelled, "invalid_connection_id").await;
}

async fn rejected(
    client: &RunningService<RoleClient, ()>,
    name: &'static str,
    arguments: Value,
    code: &str,
) {
    let response = client
        .call_tool(
            CallToolRequestParams::new(name).with_arguments(
                arguments
                    .as_object()
                    .unwrap_or_else(|| panic!("object arguments"))
                    .clone(),
            ),
        )
        .await
        .unwrap_or_else(|error| panic!("MCP response: {error}"));
    assert_eq!(response.is_error, Some(true), "{name}: {response:?}");
    assert!(
        response
            .content
            .iter()
            .filter_map(|content| content.as_text())
            .any(|text| text.text.contains(code)),
        "{name}: {response:?}"
    );
}
