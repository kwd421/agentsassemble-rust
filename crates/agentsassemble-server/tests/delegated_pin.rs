use crate::support::{human_invite, room_portal_fixture};
use agentsassemble_domain::{
    AuthenticatedPrincipal, CapabilitySet, ClientKind, InviteScope, LOCAL_OPERATOR_PARTICIPANT_ID,
    LOCAL_OPERATOR_USER_ID,
};
use agentsassemble_persistence::{RoomManagerAuthority, RoomMutationAuthority, SqliteStore};
use rmcp::{ServiceExt, model::CallToolRequestParams, transport::StreamableHttpClientTransport};
use serde_json::{Value, json};
use uuid::Uuid;

type TestResult = Result<(), Box<dyn std::error::Error>>;
type Client = rmcp::service::RunningService<rmcp::RoleClient, ()>;

pub(super) fn operator() -> AuthenticatedPrincipal {
    AuthenticatedPrincipal {
        principal_id: LOCAL_OPERATOR_USER_ID.into(),
        participant_id: LOCAL_OPERATOR_PARTICIPANT_ID.into(),
        display_name: "Host".into(),
        room_id: "general".into(),
        client_kind: ClientKind::Browser,
        invite_scope: InviteScope::ReadWrite,
        is_operator: true,
        capabilities: CapabilitySet::local_operator(ClientKind::Browser, InviteScope::ReadWrite),
    }
}

pub(super) async fn role(store: &SqliteStore, id: &str, role: &str) -> TestResult {
    store
        .execute_participant_role_update(
            RoomMutationAuthority::TrustedPrincipal(&operator()),
            &Uuid::new_v4().to_string(),
            &json!({"participant_id":id,"role":role}),
        )
        .await?;
    Ok(())
}

async fn mute(store: &SqliteStore, id: &str, muted: bool) -> TestResult {
    store
        .execute_participant_mute(
            RoomMutationAuthority::TrustedPrincipal(&operator()),
            &Uuid::new_v4().to_string(),
            &json!({"participant_id":id,"muted":muted}),
        )
        .await?;
    Ok(())
}

pub(super) async fn call(client: &Client, name: &'static str, args: Value) -> Value {
    let result = room_portal_fixture::call_tool(client, name, args).await;
    assert_ne!(result.is_error, Some(true), "{name}: {result:?}");
    serde_json::from_str(&result.content[0].as_text().unwrap().text).unwrap()
}

async fn rejected(client: &Client, args: Value, code: &str) {
    let result = client
        .call_tool(
            CallToolRequestParams::new("set_message_pin")
                .with_arguments(args.as_object().unwrap().clone()),
        )
        .await
        .unwrap();
    assert_eq!(result.is_error, Some(true));
    assert!(
        result.content[0].as_text().unwrap().text.contains(code),
        "{result:?}"
    );
}

// The same MCP mutation/replay and next-call revocation invariants run on both adapters.
pub(super) async fn verify(
    client: &Client,
    store: &SqliteStore,
    delegate: &str,
    message: &str,
    connection: Option<Value>,
    mut events: tokio::sync::broadcast::Receiver<agentsassemble_domain::RoomEvent>,
) -> TestResult {
    let mut args = json!({"channel_id":"lobby","message_id":message,"pinned":true,"request_id":Uuid::new_v4()});
    if let Some(connection) = &connection {
        args["connection_id"] = connection.clone();
    }
    if connection.is_none() {
        let tools = client.list_all_tools().await?;
        assert!(!tools.iter().any(|tool| tool.name == "set_message_pin"));
    }
    rejected(client, args.clone(), "role_required").await;
    reject_direct_tools(client, connection.as_ref(), "role_required").await;
    role(store, delegate, "director").await?;
    let tools = client.list_all_tools().await?;
    assert!(tools.iter().any(|tool| tool.name == "set_message_pin"));
    if let Some(id) = &connection {
        let status = call(client, "room_status", json!({"connection_id":id})).await;
        assert_eq!(status["role"], "director");
        assert_eq!(
            status["delegated_actions"],
            json!([
                "set_message_pin",
                "request_delete_text_channel",
                "create_text_channel",
                "rename_text_channel",
                "set_participant_mute",
                "start_agent",
                "stop_agent"
            ])
        );
    }
    verify_direct(client, store, delegate, connection.as_ref(), &mut events).await?;
    let (first, retry) = tokio::join!(
        call(client, "set_message_pin", args.clone()),
        call(client, "set_message_pin", args.clone())
    );
    assert_eq!(first, retry);
    assert_eq!(first["status"], "succeeded");
    let events = store.snapshot("general", 0, 200).await?.events;
    let attributed: Vec<_> = events
        .iter()
        .filter(|event| event.extra.get("request_id") == Some(&args["request_id"]))
        .collect();
    assert_eq!(attributed.len(), 1);
    assert_eq!(attributed[0].extra["actor_participant_id"], delegate);
    assert_eq!(
        attributed[0].extra["on_behalf_of_participant_id"],
        LOCAL_OPERATOR_PARTICIPANT_ID
    );
    assert_eq!(attributed[0].message_kind.as_deref(), Some("system"));
    assert!(
        attributed[0]
            .content
            .as_ref()
            .is_some_and(|s| s.contains("on behalf of"))
    );
    let manager = store
        .authorize_local_room_manager(
            "general",
            LOCAL_OPERATOR_USER_ID,
            LOCAL_OPERATOR_PARTICIPANT_ID,
        )
        .await?;
    assert!(
        store
            .local_message_pins(&manager, "lobby")
            .await?
            .iter()
            .any(|pin| pin.event_id == message)
    );
    let mut unknown = args.clone();
    unknown["owner_id"] = json!(LOCAL_OPERATOR_PARTICIPANT_ID);
    let result = client
        .call_tool(
            CallToolRequestParams::new("set_message_pin")
                .with_arguments(unknown.as_object().unwrap().clone()),
        )
        .await;
    assert!(result.is_err() || result.is_ok_and(|result| result.is_error == Some(true)));
    mute(store, LOCAL_OPERATOR_PARTICIPANT_ID, true).await?;
    reject_direct_tools(client, connection.as_ref(), "owner_muted").await;
    rejected(client, args.clone(), "owner_muted").await; // even a receipt retry revalidates authority
    if connection.is_none() {
        let tools = client.list_all_tools().await?;
        assert!(!tools.iter().any(|tool| tool.name == "set_message_pin"));
    } else {
        assert_eq!(
            call(client, "room_status", json!({"connection_id":connection})).await["delegated_actions"],
            json!([])
        );
    }
    mute(store, LOCAL_OPERATOR_PARTICIPANT_ID, false).await?;
    args["pinned"] = json!(false);
    args["request_id"] = json!(Uuid::new_v4());
    call(client, "set_message_pin", args.clone()).await;
    assert!(
        !store
            .local_message_pins(&manager, "lobby")
            .await?
            .iter()
            .any(|pin| pin.event_id == message)
    );
    role(store, delegate, "agent").await?;
    rejected(client, args, "role_required").await;
    reject_direct_tools(client, connection.as_ref(), "role_required").await;
    if connection.is_none() {
        let tools = client.list_all_tools().await?;
        assert!(!tools.iter().any(|tool| tool.name == "set_message_pin"));
    }
    Ok(())
}

#[tokio::test]
async fn stateless_connector_pin_uses_selected_connection_and_fresh_authority() -> TestResult {
    let (store, _) = human_invite::fixture(InviteScope::ReadWrite).await;
    let server = human_invite::start_with_public_mcp(
        store.clone(),
        "https://pin.example.test",
        "pin-fixture-proxy-00000000000000001",
    )
    .await;
    let client = ()
        .serve(StreamableHttpClientTransport::from_uri(format!(
            "{}/mcp",
            server.base_url
        )))
        .await?;
    let tools = client.list_all_tools().await?;
    assert!(tools.iter().any(|tool| tool.name == "set_message_pin"));
    let manager = RoomManagerAuthority::Local(
        store
            .authorize_local_room_manager(
                "general",
                LOCAL_OPERATOR_USER_ID,
                LOCAL_OPERATOR_PARTICIPANT_ID,
            )
            .await?,
    );
    let now = chrono::Utc::now();
    let pairing = store
        .create_operator_pairing(&manager, &[71; 32], "https://pin.example.test", now)
        .await?;
    let paired = store
        .redeem_operator_pairing(&[71; 32], &[72; 32], "https://pin.example.test", now)
        .await?;
    let invite = store
        .create_connector_invite(
            &RoomManagerAuthority::Operator(Box::new(paired.authorization)),
            Uuid::new_v4(),
            InviteScope::ReadWrite,
            chrono::Utc::now(),
        )
        .await?;
    let mut join = json!({"invite_url":format!("{}/join?token={}",server.base_url,invite.invite_bearer),"display_name":"Director"});
    join["connection_id"] = call(&client, "room_join", join.clone()).await["connection_id"].clone();
    let joined = call(&client, "room_join", join.clone()).await;
    assert_eq!(joined["role"], "agent");
    assert_eq!(joined["delegated_actions"], json!([]));
    let message = store
        .execute_message(
            &operator(),
            &Uuid::new_v4().to_string(),
            "message.send",
            &json!({"content":"Pin target"}),
        )
        .await?
        .event
        .id;
    verify(
        &client,
        &store,
        joined["participant_id"].as_str().unwrap(),
        &message,
        Some(joined["connection_id"].clone()),
        server.state().rooms.subscribe("general").await,
    )
    .await?;
    role(
        &store,
        joined["participant_id"].as_str().unwrap(),
        "director",
    )
    .await?;
    let rejoined = call(&client, "room_join", join).await;
    assert_eq!(rejoined["role"], "director");
    assert_eq!(
        rejoined["delegated_actions"],
        json!([
            "set_message_pin",
            "request_delete_text_channel",
            "create_text_channel",
            "rename_text_channel",
            "set_participant_mute",
            "start_agent",
            "stop_agent"
        ])
    );
    confirmation_socket::verify(&server, &client, &joined["connection_id"]).await?;
    store
        .revoke_operator_pairing(&manager, pairing.pairing_id)
        .await?;
    rejected(&client, json!({"connection_id":joined["connection_id"],"channel_id":"lobby","message_id":message,"pinned":true,"request_id":Uuid::new_v4()}), "owner_authority_unavailable").await;
    reject_direct_tools(
        &client,
        Some(&joined["connection_id"]),
        "owner_authority_unavailable",
    )
    .await;
    client.cancel().await?;
    server.stop().await;
    Ok(())
}

async fn verify_direct(
    client: &Client,
    store: &SqliteStore,
    delegate: &str,
    connection: Option<&Value>,
    events: &mut tokio::sync::broadcast::Receiver<agentsassemble_domain::RoomEvent>,
) -> TestResult {
    let args = |mut value: Value| {
        value["request_id"] = json!(Uuid::new_v4());
        if let Some(connection) = connection {
            value["connection_id"] = connection.clone();
        }
        value
    };
    lifecycle_tools::verify(client, store, delegate, connection, events).await?;
    let before = store.snapshot("general", 0, 200).await?.settings;
    let create = args(json!({"name":"Planning"}));
    let first = call(client, "create_text_channel", create.clone()).await;
    assert_eq!(
        first,
        call(client, "create_text_channel", create.clone()).await
    );
    assert_eq!(first["status"], "succeeded");
    let after = store.snapshot("general", 0, 200).await?.settings;
    let revision = agentsassemble_domain::public_settings(&after)?.settings_revision;
    let rename = args(
        json!({"channel_id":first["target_id"],"name":"Renamed","expected_settings_revision":revision}),
    );
    let renamed = call(client, "rename_text_channel", rename.clone()).await;
    assert_eq!(
        renamed,
        call(client, "rename_text_channel", rename.clone()).await
    );
    let mut changed = create;
    changed["name"] = json!("Conflict");
    let result = room_portal_fixture::call_tool(client, "create_text_channel", changed).await;
    assert_eq!(result.is_error, Some(true));
    assert!(
        result.content[0]
            .as_text()
            .unwrap()
            .text
            .contains("command_conflict")
    );
    let muted = args(json!({"participant_id":LOCAL_OPERATOR_PARTICIPANT_ID,"muted":true}));
    let refused = room_portal_fixture::call_tool(client, "set_participant_mute", muted).await;
    assert_eq!(refused.is_error, Some(true));
    assert!(
        refused.content[0]
            .as_text()
            .unwrap()
            .text
            .contains("owner_not_permitted")
    );
    assert!(
        !store
            .participant("general", LOCAL_OPERATOR_PARTICIPANT_ID)
            .await?
            .muted
    );
    let unmuted = call(
        client,
        "set_participant_mute",
        args(json!({"participant_id":LOCAL_OPERATOR_PARTICIPANT_ID,"muted":false})),
    )
    .await;
    for result in [first, renamed, unmuted] {
        for event in result["events"].as_array().unwrap() {
            assert_eq!(event["actor"]["participant_id"], delegate);
            assert_eq!(event["actor"]["participant_type"], "agent");
            assert_eq!(event["actor_id"], delegate);
            assert_eq!(event["actor_type"], "agent");
            assert_eq!(
                event["on_behalf_of_participant_id"],
                LOCAL_OPERATOR_PARTICIPANT_ID
            );
        }
        assert_eq!(result["event"]["actor_participant_id"], delegate);
        assert_eq!(
            result["event"]["on_behalf_of_participant_id"],
            LOCAL_OPERATOR_PARTICIPANT_ID
        );
    }
    let settings = store.snapshot("general", 0, 200).await?.settings;
    let delete = args(
        json!({"channel_id":settings.channels[0].id,"expected_settings_revision":agentsassemble_domain::public_settings(&settings)?.settings_revision}),
    );
    let pending = call(client, "request_delete_text_channel", delete.clone()).await;
    assert_eq!(pending["status"], "pending_confirmation");
    assert_eq!(settings, store.snapshot("general", 0, 200).await?.settings);
    assert_eq!(
        pending,
        call(client, "request_delete_text_channel", delete.clone()).await
    );
    let decision = agentsassemble_domain::DecideDelegatedAction {
        confirmation_id: pending["confirmation_id"].as_str().unwrap().into(),
        decision: agentsassemble_domain::ConfirmationDecision::Approve,
        full_access_acknowledged: None,
    };
    let receipt = store
        .decide_delegated_action(
            RoomMutationAuthority::TrustedPrincipal(&operator()),
            &decision,
        )
        .await?;
    assert_eq!(receipt.result["state"], "succeeded");
    assert_eq!(
        call(client, "request_delete_text_channel", delete).await,
        receipt.result["receipt"]
    );
    let completed = store.snapshot("general", 0, 200).await?;
    assert!(completed.settings.channels.is_empty());
    for event in completed
        .events
        .iter()
        .filter(|e| e.extra.get("confirmation_id") == Some(&json!(decision.confirmation_id)))
    {
        assert_eq!(event.actor.participant_id, delegate);
        assert_eq!(
            event.extra["on_behalf_of_participant_id"],
            LOCAL_OPERATOR_PARTICIPANT_ID
        );
    }
    let mut after = serde_json::to_value(store.snapshot("general", 0, 200).await?.settings)?;
    let before = serde_json::to_value(before)?;
    for field in ["channels"] {
        after[field] = before[field].clone();
    }
    assert_eq!(before, after);
    Ok(())
}

#[path = "delegated_lifecycle_tools.rs"]
mod lifecycle_tools;

async fn reject_direct_tools(client: &Client, connection: Option<&Value>, code: &str) {
    for (name, mut payload) in [
        ("create_text_channel", json!({"name":"Denied"})),
        (
            "rename_text_channel",
            json!({"channel_id":"missing","name":"Denied","expected_settings_revision":"stale"}),
        ),
        (
            "set_participant_mute",
            json!({"participant_id":"missing","muted":true}),
        ),
        ("start_agent", json!({"agent_id":"missing"})),
        ("stop_agent", json!({"agent_id":"missing"})),
    ] {
        payload["request_id"] = json!(Uuid::new_v4());
        if let Some(connection) = connection {
            payload["connection_id"] = connection.clone();
        }
        let result = room_portal_fixture::call_tool(client, name, payload).await;
        assert_eq!(result.is_error, Some(true));
        assert!(
            result.content[0].as_text().unwrap().text.contains(code),
            "{name}: {result:?}"
        );
    }
}

#[path = "delegated_confirmation_socket.rs"]
mod confirmation_socket;
