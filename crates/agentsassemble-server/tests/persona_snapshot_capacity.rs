use agentsassemble_domain::{
    AgentSessionDraft, AuthenticatedPrincipal, CapabilitySet, ClientKind, InviteScope,
    LOCAL_OPERATOR_PARTICIPANT_ID, LOCAL_OPERATOR_USER_ID,
};
use agentsassemble_persistence::{
    RoomMutationAuthority::TrustedPrincipal, SqliteStore, import_ccv3_asset,
};
use agentsassemble_server::issue_local_ticket;
use serde_json::{Value, json};
mod support {
    pub mod human_invite;
    pub mod room_socket_peer;
}

#[tokio::test]
async fn full_persona_capacity_create_configure_and_reopen_fit_actual_socket()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("runtime.sqlite3");
    let store = SqliteStore::open_path(&path).await?;
    store
        .bootstrap_local_authority("90b4b9d3-c12e-4495-9955-f0f70d44e55c", "Host")
        .await?;
    store
        .create_room_for_local_operator(
            "20000000-0000-4000-8000-000000000001",
            "general",
            "General",
        )
        .await?;
    let principal = AuthenticatedPrincipal {
        principal_id: LOCAL_OPERATOR_USER_ID.into(),
        participant_id: LOCAL_OPERATOR_PARTICIPANT_ID.into(),
        display_name: "Host".into(),
        room_id: "general".into(),
        client_kind: ClientKind::Browser,
        invite_scope: InviteScope::ReadWrite,
        is_operator: true,
        capabilities: CapabilitySet::local_operator(ClientKind::Browser, InviteScope::ReadWrite),
    };
    // Escapes count toward encoded bytes; each selected summary is near the 1KiB ceiling.
    let name = format!("{}{}", "A".repeat(80), "\"X".repeat(160));
    let imported = import_ccv3_asset(
        "guide.json",
        json!({"spec":"chara_card_v3","data":{"name":name}})
            .to_string()
            .into_bytes(),
    )
    .await?;
    let summary = store.replace_persona_asset(imported).await?;
    assert!(serde_json::to_vec(&summary)?.len() > 800);
    let mut first = None;
    for i in 0..64 {
        let draft = AgentSessionDraft {
            agent_id: format!("api-{i}"),
            display_name: format!("Agent {i}"),
            provider_kind: "deepseek_api".into(),
            runtime_kind: "api".into(),
            connection_kind: "direct_api".into(),
            executable: String::new(),
            executable_identity: String::new(),
            workspace: String::new(),
            workspace_identity: String::new(),
            provider_endpoint: String::new(),
            model: "deepseek-v4-flash".into(),
            reasoning_effort: String::new(),
            service_tier: "default".into(),
            variant: String::new(),
            execution_harness: "builtin".into(),
            permission_mode: "meeting_read_only".into(),
            max_output_tokens: 0,
            catalog_revision: "test".into(),
            persona_card_id: summary.id.clone(),
            runtime_profile_key: format!("profile-{i}"),
            transport: "http".into(),
        };
        let created = store
            .execute_agent_create(
                TrustedPrincipal(&principal),
                &format!("create-{i}"),
                &json!({"agent_id":draft.agent_id}),
                &draft,
            )
            .await?;
        assert!(
            serde_json::to_vec(&created.result)?.len()
                < agentsassemble_protocol::MAX_ROOM_SOCKET_MESSAGE_BYTES
        );
        if i == 0 {
            first = Some(draft);
        }
    }
    let mut draft = first.ok_or("missing draft")?;
    let old_key = draft.runtime_profile_key.clone();
    draft.runtime_profile_key = "configured".into();
    let configured = store
        .execute_agent_configuration(
            TrustedPrincipal(&principal),
            "configure",
            &json!({"agent_id":draft.agent_id}),
            &old_key,
            &draft,
        )
        .await?;
    assert!(
        serde_json::to_vec(&configured.result)?.len()
            < agentsassemble_protocol::MAX_ROOM_SOCKET_MESSAGE_BYTES
    );
    verify_socket(store.clone()).await?;
    drop(store);
    verify_socket(SqliteStore::open_path(&path).await?).await?;
    Ok(())
}

async fn verify_socket(store: SqliteStore) -> Result<(), Box<dyn std::error::Error>> {
    let server = support::human_invite::start(store).await;
    let ticket = issue_local_ticket(server.state(), "general").await?;
    let (socket, _) = tokio_tungstenite::connect_async(format!(
        "{}/ws?ticket={}",
        server.base_url.replacen("http://", "ws://", 1),
        ticket.ticket
    ))
    .await?;
    let mut socket = support::room_socket_peer::RoomSocketPeer::new(socket);
    assert_eq!(socket.subscribe(0).await["op"], "subscribed");
    let wire = socket.receive_text().await;
    let snapshot: Value = serde_json::from_str(&wire)?;
    assert_eq!(snapshot["op"], "snapshot");
    assert_eq!(
        snapshot["agent_sessions"].as_array().map(Vec::len),
        Some(64)
    );
    assert!(wire.len() <= agentsassemble_protocol::MAX_ROOM_SOCKET_MESSAGE_BYTES);
    socket.close().await;
    server.stop().await;
    Ok(())
}
