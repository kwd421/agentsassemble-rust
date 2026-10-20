use super::{conversation_workspace, driver_model, key_broker};
use crate::opencode_protocol::validate_profile;
use crate::test_support::durable_session;

const GEMINI_SECRET: &str = "gemini-harness-private-value";

#[tokio::test]
async fn the_api_key_stays_in_the_broker_and_never_reaches_opencode_config() {
    let session = durable_session(
        "room",
        "gemini-harness",
        "Gemini",
        "gemini_api",
        "gemini-3.1-flash-lite",
        "https",
    );
    let broker = key_broker(&session, GEMINI_SECRET)
        .await
        .unwrap_or_else(|error| panic!("start Gemini key broker: {error}"));
    let config = broker.config_content();
    assert!(!config.contains(GEMINI_SECRET));
    assert!(config.contains("\"small_model\":\"google/gemini-3.1-flash-lite\""));

    let unsupported = durable_session("room", "x", "X", "openrouter_api", "model", "https");
    let error = key_broker(&unsupported, GEMINI_SECRET)
        .await
        .err()
        .unwrap_or_else(|| panic!("unsupported harness provider must fail"));
    assert_eq!(error.code, "invalid_runtime_profile");
    assert!(!error.message.contains(GEMINI_SECRET));
}

#[test]
fn stored_models_map_to_the_documented_opencode_ids() {
    assert_eq!(
        driver_model("gemini_api", "gemini-3.1-flash-lite").as_deref(),
        Some("google/gemini-3.1-flash-lite")
    );
    assert_eq!(
        driver_model("deepseek_api", "deepseek-chat").as_deref(),
        Some("deepseek/deepseek-chat")
    );
    for invalid in ["", "../secret", "google/gemini", "gemini 3"] {
        assert_eq!(driver_model("gemini_api", invalid), None, "{invalid}");
    }
    assert_eq!(driver_model("openai_api", "gpt"), None);
}

#[test]
fn the_credential_never_appears_in_opencode_arguments() {
    let arguments = crate::opencode::server_arguments(43123);
    assert_eq!(
        arguments,
        ["serve", "--hostname", "127.0.0.1", "--port", "43123"]
    );
    assert!(!arguments.iter().any(|value| value.contains(GEMINI_SECRET)));
}

#[test]
fn harness_profile_accepts_the_mapped_model_without_loosening_real_sessions() {
    let mut harness = durable_session(
        "room",
        "gemini-harness",
        "Gemini",
        "gemini_api",
        "gemini-3.1-flash-lite",
        "https",
    );
    harness.public.execution_harness = "opencode".to_owned();
    harness.public.reasoning_effort.clear();
    harness.public.service_tier.clear();
    harness.public.variant.clear();
    validate_profile(&harness)
        .unwrap_or_else(|error| panic!("accept mapped Gemini model: {error}"));

    let mut unsupported_controls = harness.clone();
    unsupported_controls.public.service_tier = "default".to_owned();
    assert!(validate_profile(&unsupported_controls).is_err());

    let mut real = durable_session(
        "room",
        "real-opencode",
        "OpenCode",
        "opencode_server",
        "google/gemini-3.1-flash-lite",
        "http",
    );
    real.public.runtime_kind = "opencode".to_owned();
    real.public.reasoning_effort.clear();
    assert!(
        validate_profile(&real).is_err(),
        "real OpenCode sessions must not accept API provider ids"
    );
}

#[tokio::test]
async fn a_conversation_agent_keeps_one_private_folder_across_launches() {
    let state = tempfile::tempdir().unwrap_or_else(|error| panic!("state root: {error}"));
    let session = durable_session("room", "agent", "Gemini", "gemini_api", "model", "https");
    let first = conversation_workspace(Some(state.path()), &session)
        .await
        .unwrap_or_else(|error| panic!("first launch folder: {error:?}"))
        .unwrap_or_else(|| panic!("a conversation agent needs a folder"));
    let second = conversation_workspace(Some(state.path()), &session)
        .await
        .unwrap_or_else(|error| panic!("second launch folder: {error:?}"));
    assert_eq!(second.as_ref(), Some(&first));
    assert!(first.starts_with(std::fs::canonicalize(state.path()).unwrap_or_default()));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&first)
            .unwrap_or_else(|error| panic!("folder metadata: {error}"))
            .permissions()
            .mode();
        assert_eq!(mode & 0o077, 0);
    }

    let other = durable_session("room", "other", "Gemini", "gemini_api", "model", "https");
    let other = conversation_workspace(Some(state.path()), &other)
        .await
        .unwrap_or_else(|error| panic!("other agent folder: {error:?}"));
    assert_ne!(other.as_ref(), Some(&first));

    let mut owned = session.clone();
    owned.workspace = "/owner/selected".to_owned();
    let owned = conversation_workspace(None, &owned)
        .await
        .unwrap_or_else(|error| panic!("owner workspace: {error:?}"));
    assert_eq!(owned, None);
    assert!(conversation_workspace(None, &session).await.is_err());
}
