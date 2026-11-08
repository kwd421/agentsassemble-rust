//! The host's startable harnesses and models, read by a director AI so its
//! `request_create_agent`/`request_reconfigure_agent` can name exact catalog values.
use agentsassemble_domain::ProviderCatalog;
use agentsassemble_persistence::{DelegateRoomAuthority, PersistenceError, SqliteStore};
use serde_json::{Value, json};

/// Serves the catalog only while the AI's current director authority offers agent
/// creation or reconfiguration. Executable paths and credential state never appear.
/// # Errors
/// Rejects `delegated_action_unavailable` without that authority; propagates storage failures.
pub(crate) async fn read(
    store: &SqliteStore,
    catalog: &ProviderCatalog,
    authority: &DelegateRoomAuthority,
) -> Result<Value, PersistenceError> {
    let allowed = store.delegated_direct_actions(authority).await?;
    if !allowed
        .iter()
        .any(|action| action == "request_create_agent" || action == "request_reconfigure_agent")
    {
        return Err(PersistenceError::CommandRejected {
            code: "delegated_action_unavailable".into(),
            message: "Reading the agent catalog requires current director authority over agents."
                .into(),
        });
    }
    let providers: Vec<Value> = catalog
        .providers
        .iter()
        .filter(|provider| provider.available && provider.startable)
        .map(|provider| {
            let controls: Vec<Value> = provider
                .controls
                .iter()
                .map(|control| {
                    let options: Vec<Value> = control
                        .options
                        .iter()
                        .map(|option| json!({"value": option.value, "label": option.label}))
                        .collect();
                    json!({"key": control.key, "default_value": control.default_value, "options": options})
                })
                .collect();
            json!({"provider_id": provider.id, "display_name": provider.display_name,
                "workspace_required": provider.workspace_required,
                "default_model": provider.default_model, "controls": controls})
        })
        .collect();
    Ok(json!({
        "catalog_revision": catalog.catalog_revision,
        "status": catalog.status,
        "providers": providers,
    }))
}
