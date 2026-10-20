use super::{driver_model, environment};
use crate::opencode_protocol::validate_profile;
use crate::test_support::durable_session;

const GEMINI_SECRET: &str = "gemini-harness-private-value";

#[test]
fn each_supported_provider_adds_only_its_own_credential_variable() {
    assert_eq!(
        environment("gemini_api", GEMINI_SECRET)
            .unwrap_or_else(|error| panic!("build Gemini harness environment: {error}")),
        vec![(
            "GOOGLE_GENERATIVE_AI_API_KEY".to_owned(),
            GEMINI_SECRET.to_owned()
        )]
    );
    assert_eq!(
        environment("deepseek_api", "deepseek-harness-private-value")
            .unwrap_or_else(|error| panic!("build DeepSeek harness environment: {error}")),
        vec![(
            "DEEPSEEK_API_KEY".to_owned(),
            "deepseek-harness-private-value".to_owned()
        )]
    );
    let error = environment("openrouter_api", GEMINI_SECRET)
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
