use agentsassemble_domain::{
    AgentLifecycleAction, AgentLifecycleIntentStatus, AgentRuntimeStatus, AgentSession,
    AgentSessionStatus, AgentTurnPhase, CURRENT_RUNTIME_PROFILE_VERSION, DurableAgentSession,
};
use chrono::{DateTime, Utc};

pub(crate) fn durable_session(
    room_id: &str,
    session_id: &str,
    display_name: &str,
    provider_kind: &str,
    model: &str,
    transport: &str,
) -> DurableAgentSession {
    let now = DateTime::parse_from_rfc3339("2026-08-23T00:00:00Z")
        .unwrap_or_else(|error| panic!("parse fixture timestamp: {error}"))
        .with_timezone(&Utc);
    DurableAgentSession {
        public: AgentSession {
            room_id: room_id.to_owned(),
            session_id: session_id.to_owned(),
            participant_id: session_id.to_owned(),
            display_name: display_name.to_owned(),
            status: AgentSessionStatus::Available,
            runtime_status: AgentRuntimeStatus::Starting,
            enabled: true,
            provider_kind: provider_kind.to_owned(),
            runtime_kind: "live_cli".to_owned(),
            connection_kind: "native_cli_bridge".to_owned(),
            external_owned: false,
            process_ownership: "server".to_owned(),
            model: model.to_owned(),
            reasoning_effort: "high".to_owned(),
            service_tier: String::new(),
            variant: String::new(),
            execution_harness: "builtin".to_owned(),
            permission_mode: "meeting_read_only".to_owned(),
            max_output_tokens: 0,
            catalog_revision: "revision".to_owned(),
            persona_card_id: Box::default(),
            persona_card: None,
            transport: transport.to_owned(),
            last_seen_event_id: String::new(),
            last_seen_seq: 0,
            last_provider_sync_event_id: String::new(),
            last_provider_sync_seq: 0,
            bootstrap_cutoff_seq: 0,
            turn_count: 0,
            active_turn_id: String::new(),
            turn_phase: AgentTurnPhase::None,
            last_error: String::new(),
            last_error_code: String::new(),
            recovery_required: false,
            provider_session_active: false,
            provider_session_reused: false,
            created_at: now,
            updated_at: now,
        },
        executable: String::new(),
        executable_identity: String::new(),
        workspace: String::new(),
        workspace_identity: String::new(),
        provider_endpoint: String::new(),
        runtime_profile_key: "profile".to_owned(),
        runtime_profile_version: CURRENT_RUNTIME_PROFILE_VERSION,
        provider_session_id: String::new(),
        runtime_handle_id: String::new(),
        runtime_owner_id: String::new(),
        runtime_lease_token: String::new(),
        turn_generation: 0,
        schedule_requested: false,
        pending_inputs: Vec::new(),
        inflight_inputs: Vec::new(),
        active_source_event_id: String::new(),
        input_up_to_event_id: String::new(),
        input_up_to_seq: 0,
        lifecycle_intent_action: AgentLifecycleAction::None,
        lifecycle_intent_id: String::new(),
        lifecycle_intent_status: AgentLifecycleIntentStatus::None,
    }
}

pub(crate) async fn assert_default_tier_selection(
    provider_id: &str,
    default_model: String,
    controls: Vec<agentsassemble_domain::ProviderControl>,
) {
    use crate::{catalog::ready_provider, registration, selection::ProviderSelection};
    use serde_json::json;

    let registration = registration::provider_registration_by_id(provider_id)
        .unwrap_or_else(|| panic!("registered provider"));
    let mut provider = registration::loading_provider(registration);
    provider.executable = std::env::current_exe()
        .and_then(std::fs::canonicalize)
        .unwrap_or_else(|error| panic!("resolve test executable: {error}"))
        .to_string_lossy()
        .into_owned();
    provider.executable_identity = crate::filesystem::runtime_executable_identity(
        &provider.provider_kind,
        provider.executable.clone(),
    )
    .await
    .unwrap_or_else(|error| panic!("identify test executable: {error:?}"));
    let provider = ready_provider(provider, default_model, controls);
    assert!(provider.startable);
    let catalog = agentsassemble_domain::ProviderCatalog {
        status: "ready".to_owned(),
        catalog_revision: "no-fast".to_owned(),
        discovered_at: String::new(),
        providers: vec![provider],
    };
    let workspace = tempfile::tempdir().unwrap_or_else(|error| panic!("workspace: {error}"));
    let mut payload = json!({
        "provider_id": provider_id,
        "catalog_revision": "no-fast",
        "display_name": "Default tier",
        "workspace": workspace.path(),
    });
    for tier in [None, Some("default"), Some("fast")] {
        if let Some(tier) = tier {
            payload["service_tier"] = json!(tier);
        }
        let result =
            ProviderSelection::from_catalog("room", "operator", "create", &payload, &catalog).await;
        if tier == Some("fast") {
            assert_eq!(
                result
                    .err()
                    .unwrap_or_else(|| panic!("unadvertised fast tier must fail"))
                    .code,
                "unsupported_control"
            );
        } else {
            assert_eq!(
                result
                    .unwrap_or_else(|error| panic!("select default tier: {error}"))
                    .service_tier,
                "default"
            );
        }
    }
}
