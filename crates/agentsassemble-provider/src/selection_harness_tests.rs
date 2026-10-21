use agentsassemble_domain::{ProviderAvailability, ProviderCatalog};
use serde_json::{Value, json};

use super::super::ProviderSelection;
use crate::catalog::{control, option, permission_control, ready_provider};
use crate::registration::{
    CODEX_PROVIDER, DEEPSEEK_PROVIDER, GEMINI_PROVIDER, OPENCODE_PROVIDER, loading_provider,
};

const REVISION: &str = "catalog-open-code";

async fn executable() -> (String, String) {
    let path = std::env::current_exe()
        .and_then(std::fs::canonicalize)
        .unwrap_or_else(|error| panic!("resolve test executable: {error}"));
    let identity = crate::filesystem::runtime_executable_identity(
        "opencode_server",
        path.to_string_lossy().into_owned(),
    )
    .await
    .unwrap_or_else(|error| panic!("identify test executable: {error:?}"));
    (path.to_string_lossy().into_owned(), identity)
}

fn gemini() -> ProviderAvailability {
    ready_provider(
        loading_provider(&GEMINI_PROVIDER),
        "gemini-3.1-flash-lite".to_owned(),
        vec![
            control(
                "model",
                "모델",
                "combobox",
                vec![option("gemini-3.1-flash-lite", "Gemini 3.1 Flash Lite")],
                "gemini-3.1-flash-lite",
            ),
            control(
                "service_tier",
                "응답 속도",
                "select",
                vec![option("default", "기본"), option("flex", "Flex")],
                "default",
            ),
            permission_control(true, false),
        ],
    )
}

fn deepseek() -> ProviderAvailability {
    let mut model = option("deepseek-v4-flash", "deepseek-v4-flash");
    model
        .metadata
        .insert("relation_scope".to_owned(), json!("global"));
    model
        .metadata
        .insert("reasoning_efforts".to_owned(), json!(["low", "high"]));
    ready_provider(
        loading_provider(&DEEPSEEK_PROVIDER),
        "deepseek-v4-flash".to_owned(),
        vec![
            control(
                "model",
                "모델",
                "combobox",
                vec![model],
                "deepseek-v4-flash",
            ),
            control(
                "reasoning_effort",
                "추론 강도",
                "select",
                vec![option("low", "low"), option("high", "high")],
                "high",
            ),
            control(
                "variant",
                "Thinking",
                "select",
                vec![
                    option("thinking", "사용"),
                    option("non_thinking", "사용 안 함"),
                ],
                "thinking",
            ),
            permission_control(true, false),
        ],
    )
}

async fn opencode() -> ProviderAvailability {
    let (path, identity) = executable().await;
    let mut provider = loading_provider(&OPENCODE_PROVIDER);
    provider.executable = path;
    provider.executable_identity = identity;
    provider.available = true;
    provider.startable = true;
    "ready".clone_into(&mut provider.discovery_status);
    provider
}

fn catalog(providers: Vec<ProviderAvailability>) -> ProviderCatalog {
    ProviderCatalog {
        status: "ready".to_owned(),
        catalog_revision: REVISION.to_owned(),
        discovered_at: String::new(),
        providers,
    }
}

fn gemini_payload(harness: &str) -> Value {
    json!({
        "provider_id": "gemini",
        "catalog_revision": REVISION,
        "display_name": "Gemini",
        "workspace": "",
        "model": "gemini-3.1-flash-lite",
        "execution_harness": harness,
        "permission_mode": "meeting_read_only"
    })
}

async fn select(
    payload: &Value,
    authority: &ProviderCatalog,
) -> Result<ProviderSelection, super::super::ProviderSelectionError> {
    ProviderSelection::from_catalog("room", "operator", "create-harness", payload, authority).await
}

#[tokio::test]
async fn gemini_and_deepseek_accept_opencode_with_the_catalog_executable() {
    let opencode = opencode().await;
    let authority = catalog(vec![gemini(), deepseek(), opencode.clone()]);
    let selected = select(&gemini_payload("opencode"), &authority)
        .await
        .unwrap_or_else(|error| panic!("select Gemini harness: {error}"));
    assert_eq!(selected.provider_kind, "gemini_api");
    assert_eq!(selected.model, "gemini-3.1-flash-lite");
    assert_eq!(selected.execution_harness, "opencode");
    assert_eq!(selected.executable, opencode.executable);
    assert_eq!(selected.executable_identity, opencode.executable_identity);
    assert!(selected.workspace.is_empty());
    assert!(selected.service_tier.is_empty());

    let selected = select(
        &json!({
            "provider_id": "deepseek",
            "catalog_revision": REVISION,
            "display_name": "DeepSeek",
            "workspace": "",
            "model": "deepseek-v4-flash",
            "execution_harness": "opencode",
            "reasoning_effort": "high",
            "variant": "thinking",
            "permission_mode": "meeting_read_only"
        }),
        &authority,
    )
    .await
    .unwrap_or_else(|error| panic!("select DeepSeek harness: {error}"));
    assert_eq!(selected.provider_kind, "deepseek_api");
    assert_eq!(selected.model, "deepseek-v4-flash");
    assert_eq!(selected.execution_harness, "opencode");
    assert!(selected.reasoning_effort.is_empty());
    assert!(selected.service_tier.is_empty());
    assert!(selected.variant.is_empty());
    assert_eq!(selected.executable, opencode.executable);

    let reconfigured = select(
        &json!({
            "provider_id": "deepseek",
            "catalog_revision": REVISION,
            "display_name": "DeepSeek",
            "workspace": "",
            "model": "deepseek-v4-flash",
            "execution_harness": "opencode",
            "reasoning_effort": "",
            "variant": "",
            "permission_mode": "meeting_read_only"
        }),
        &authority,
    )
    .await
    .unwrap_or_else(|error| panic!("keep stored harness controls empty: {error}"));
    assert_eq!(reconfigured.execution_harness, "opencode");
    assert!(reconfigured.reasoning_effort.is_empty());
    assert!(reconfigured.variant.is_empty());
}

#[tokio::test]
async fn builtin_and_empty_harness_keep_the_original_executable_authority() {
    let authority = catalog(vec![gemini()]);
    for harness in ["", "builtin"] {
        let selected = select(&gemini_payload(harness), &authority)
            .await
            .unwrap_or_else(|error| panic!("select builtin harness: {error}"));
        assert_eq!(selected.execution_harness, "builtin");
        assert!(selected.executable.is_empty());
        assert!(selected.executable_identity.is_empty());
    }
}

#[tokio::test]
async fn other_providers_and_harness_values_are_refused_with_the_existing_error() {
    let (path, identity) = executable().await;
    let mut codex = loading_provider(&CODEX_PROVIDER);
    codex.executable = path;
    codex.executable_identity = identity;
    codex.available = true;
    codex.startable = true;
    "ready".clone_into(&mut codex.discovery_status);
    let mut codex_model = option("gpt-5.6-terra", "gpt-5.6-terra");
    codex_model
        .metadata
        .insert("relation_scope".to_owned(), json!("global"));
    let codex = ready_provider(
        codex,
        "gpt-5.6-terra".to_owned(),
        vec![control(
            "model",
            "모델",
            "combobox",
            vec![codex_model],
            "gpt-5.6-terra",
        )],
    );
    let authority = catalog(vec![codex, gemini(), opencode().await]);
    let mut payload = gemini_payload("opencode");
    payload["provider_id"] = json!("codex");
    payload["model"] = json!("gpt-5.6-terra");
    for (payload, code) in [
        (payload, "unsupported_control"),
        (gemini_payload("shell"), "unsupported_control"),
    ] {
        let error = select(&payload, &authority)
            .await
            .err()
            .unwrap_or_else(|| panic!("harness must be refused"));
        assert_eq!(error.code, code);
        assert_eq!(
            error.message,
            "Alternate execution harnesses are not available in this runtime slice."
        );
    }
}

#[tokio::test]
async fn missing_unready_or_changed_opencode_is_refused() {
    let error = select(&gemini_payload("opencode"), &catalog(vec![gemini()]))
        .await
        .err()
        .unwrap_or_else(|| panic!("missing OpenCode must fail"));
    assert_eq!(error.code, "opencode_harness_unavailable");

    let mut unready = opencode().await;
    unready.startable = false;
    "loading".clone_into(&mut unready.discovery_status);
    let error = select(
        &gemini_payload("opencode"),
        &catalog(vec![gemini(), unready]),
    )
    .await
    .err()
    .unwrap_or_else(|| panic!("unready OpenCode must fail"));
    assert_eq!(error.code, "opencode_harness_unavailable");

    let mut stale = opencode().await;
    stale.executable_identity = "stale-identity".to_owned();
    let error = select(&gemini_payload("opencode"), &catalog(vec![gemini(), stale]))
        .await
        .err()
        .unwrap_or_else(|| panic!("stale OpenCode identity must fail"));
    assert_eq!(error.code, "catalog_changed");
}

/// OpenCode allows everything for the whole session, so it cannot keep the
/// built-in harness's rule that turns answering someone else ask first.
#[tokio::test]
async fn full_access_is_refused_behind_the_opencode_harness() {
    let opencode = opencode().await;
    let mut provider = gemini();
    provider
        .controls
        .retain(|control| control.key != "permission_mode");
    provider.controls.push(permission_control(true, true));
    let authority = catalog(vec![provider, opencode]);
    let mut payload = gemini_payload("opencode");
    payload["permission_mode"] = json!("full_access");
    payload["full_access_acknowledged"] = json!(true);
    let error = select(&payload, &authority)
        .await
        .err()
        .unwrap_or_else(|| panic!("full access behind OpenCode must be refused"));
    assert_eq!(error.code, "full_access_unsupported_harness");
}
