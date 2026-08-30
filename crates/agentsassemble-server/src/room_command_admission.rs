use agentsassemble_domain::{AuthenticatedPrincipal, canonical_payload_hash};
use agentsassemble_persistence::{
    HumanSessionAuthorization, PersistenceError, SqliteStore, room_write_command_size,
};
use agentsassemble_protocol::RoomAction;
use serde_json::Value;
use tokio::sync::OwnedSemaphorePermit;

use crate::{
    principal_mutation_admission::{MutationDebit, MutationIdentity, PrincipalMutationAdmission},
    room_command_result::{CommandFailure, validate_command_envelope},
};

pub(crate) struct AdmittedHumanCommand {
    pub(crate) principal: AuthenticatedPrincipal,
    pub(crate) mutation_debit: Option<MutationDebit>,
    pub(crate) inflight_permit: OwnedSemaphorePermit,
}

pub(crate) async fn admit_human_command(
    store: &SqliteStore,
    admission: &PrincipalMutationAdmission,
    principal: &AuthenticatedPrincipal,
    request_id: &str,
    action: RoomAction,
    payload: &Value,
) -> Result<AdmittedHumanCommand, CommandFailure> {
    validate_command_envelope(request_id).map_err(CommandFailure::rejected)?;
    let principal = store
        .resolve_principal(principal)
        .await
        .map_err(CommandFailure::unresolved)?;
    admit_current_command(store, admission, principal, request_id, action, payload).await
}

pub(crate) async fn admit_human_session_command(
    store: &SqliteStore,
    admission: &PrincipalMutationAdmission,
    authorization: &HumanSessionAuthorization,
    request_id: &str,
    action: RoomAction,
    payload: &Value,
) -> Result<(AdmittedHumanCommand, HumanSessionAuthorization), CommandFailure> {
    validate_command_envelope(request_id).map_err(CommandFailure::rejected)?;
    let current = store
        .revalidate_human_session_authorization(authorization)
        .await
        .map_err(CommandFailure::unresolved)?;
    let admitted = admit_current_command(
        store,
        admission,
        current.principal().clone(),
        request_id,
        action,
        payload,
    )
    .await?;
    Ok((admitted, current))
}

async fn admit_current_command(
    store: &SqliteStore,
    admission: &PrincipalMutationAdmission,
    principal: AuthenticatedPrincipal,
    request_id: &str,
    action: RoomAction,
    payload: &Value,
) -> Result<AdmittedHumanCommand, CommandFailure> {
    let action_name = action.as_str();
    let payload_bytes = room_write_command_size(request_id, action_name, payload)
        .map_err(CommandFailure::unresolved)?;
    let payload_hash = canonical_payload_hash(payload);
    let identity =
        MutationIdentity::new(&principal.room_id, request_id, action_name, &payload_hash);
    let mutation_debit = match store
        .command_requires_principal_budget(&principal, request_id, action_name, payload)
        .await
    {
        Ok(true) => Some(
            admission
                .charge(&principal.principal_id, identity, payload_bytes)
                .map_err(CommandFailure::after_admission)?,
        ),
        Ok(false) => None,
        Err(error) if admission_error_is_definitive(&error) => {
            let debit = admission
                .charge(&principal.principal_id, identity, payload_bytes)
                .map_err(CommandFailure::after_admission)?;
            debit.resolve();
            return Err(CommandFailure::transactional(error));
        }
        Err(error) => return Err(CommandFailure::unresolved(error)),
    };
    let inflight_permit = admission
        .acquire_inflight()
        .map_err(CommandFailure::after_admission)?;
    Ok(AdmittedHumanCommand {
        principal,
        mutation_debit,
        inflight_permit,
    })
}

fn admission_error_is_definitive(error: &PersistenceError) -> bool {
    matches!(
        error,
        PersistenceError::CommandConflict
            | PersistenceError::CommandRejected { .. }
            | PersistenceError::StoredCommandRejected { .. }
            | PersistenceError::ParticipantMissing
            | PersistenceError::RoomMissing
    )
}

#[cfg(test)]
mod tests {
    use std::{fs::File, path::Path};

    use agentsassemble_domain::{
        AgentSessionDraft, AuthenticatedPrincipal, CapabilitySet, ClientKind, InviteScope,
        LOCAL_OPERATOR_PARTICIPANT_ID, stable_content_identity, stable_identity_hash,
    };
    use agentsassemble_persistence::{AgentStartPlan, SqliteStore};
    use agentsassemble_protocol::RoomAction;
    use same_file::Handle;
    use serde_json::json;

    use super::{PrincipalMutationAdmission, admit_human_command};

    const AGENT_ID: &str = "codex-00000000-0000-5000-8000-000000000001";

    #[tokio::test]
    async fn terminal_lifecycle_replay_receives_a_new_process_debit() {
        let (store, principal, _directory) = fixture().await;
        let payload = json!({"agent_id": AGENT_ID});
        let admission = PrincipalMutationAdmission::new();
        let first = admit_human_command(
            &store,
            &admission,
            &principal,
            "terminal-rejection",
            RoomAction::AgentStart,
            &payload,
        )
        .await
        .unwrap_or_else(|failure| panic!("admit fresh start: {}", failure.error));
        first
            .mutation_debit
            .as_ref()
            .unwrap_or_else(|| panic!("fresh start must receive a process debit"))
            .resolve();
        drop(first);

        let AgentStartPlan::Start(effect) = store
            .prepare_agent_start(&principal, "terminal-rejection", &payload)
            .await
            .unwrap_or_else(|error| panic!("prepare failed start: {error}"))
        else {
            panic!("stopped session must prepare a start effect");
        };
        store
            .fail_agent_start_before_effect(
                &principal,
                "terminal-rejection",
                &payload,
                &effect.operation_id,
                "runtime_start_failed",
                "Provider runtime could not start.",
                "agent.start",
            )
            .await
            .unwrap_or_else(|error| panic!("record terminal rejection: {error}"));

        let replay = admit_human_command(
            &store,
            &admission,
            &principal,
            "terminal-rejection",
            RoomAction::AgentStart,
            &payload,
        )
        .await
        .unwrap_or_else(|failure| panic!("admit rejected replay: {}", failure.error));
        assert!(
            replay.mutation_debit.is_some(),
            "a terminal rejection closes the retry exemption and must receive a new debit"
        );
    }

    async fn fixture() -> (SqliteStore, AuthenticatedPrincipal, tempfile::TempDir) {
        let directory =
            tempfile::tempdir().unwrap_or_else(|error| panic!("create test directory: {error}"));
        let store = SqliteStore::open_path(&directory.path().join("runtime.sqlite3"))
            .await
            .unwrap_or_else(|error| panic!("open store: {error}"));
        store
            .bootstrap_local_authority("42aebf93-31ce-46fd-b792-0a791b644668", "Host")
            .await
            .unwrap_or_else(|error| panic!("bootstrap identity: {error}"));
        store
            .create_room_for_local_operator(
                "20000000-0000-4000-8000-000000000010",
                "general",
                "General",
            )
            .await
            .unwrap_or_else(|error| panic!("create room: {error}"));
        let principal = AuthenticatedPrincipal {
            principal_id: "operator-local-user".to_owned(),
            participant_id: LOCAL_OPERATOR_PARTICIPANT_ID.to_owned(),
            display_name: "Host".to_owned(),
            room_id: "general".to_owned(),
            client_kind: ClientKind::Browser,
            invite_scope: InviteScope::ReadWrite,
            is_operator: true,
            capabilities: CapabilitySet::local_operator(
                ClientKind::Browser,
                InviteScope::ReadWrite,
            ),
        };
        let create_payload = json!({"provider_id": "codex", "catalog_revision": "catalog-1"});
        store
            .execute_agent_create(
                &principal,
                "create-agent",
                &create_payload,
                &draft(directory.path()),
            )
            .await
            .unwrap_or_else(|error| panic!("create agent: {error}"));
        (store, principal, directory)
    }

    fn draft(workspace: &Path) -> AgentSessionDraft {
        let workspace = workspace
            .canonicalize()
            .unwrap_or_else(|error| panic!("canonical workspace: {error}"));
        let executable = std::env::current_exe()
            .and_then(std::fs::canonicalize)
            .unwrap_or_else(|error| panic!("canonical executable: {error}"));
        let mut file = File::open(&executable)
            .unwrap_or_else(|error| panic!("open executable authority: {error}"));
        let executable_handle = Handle::from_file(
            file.try_clone()
                .unwrap_or_else(|error| panic!("clone executable authority: {error}")),
        )
        .unwrap_or_else(|error| panic!("identify executable authority: {error}"));
        let executable_identity = stable_content_identity(&executable_handle, &mut file)
            .unwrap_or_else(|error| panic!("hash executable authority: {error}"));
        AgentSessionDraft {
            agent_id: AGENT_ID.to_owned(),
            display_name: "Terra".to_owned(),
            provider_kind: "opencode_server".to_owned(),
            runtime_kind: "live_cli".to_owned(),
            connection_kind: "native_cli_bridge".to_owned(),
            executable: executable.to_string_lossy().into_owned(),
            executable_identity,
            workspace: workspace.to_string_lossy().into_owned(),
            workspace_identity: stable_identity_hash(
                &Handle::from_path(&workspace)
                    .unwrap_or_else(|error| panic!("identify workspace: {error}")),
            ),
            model: "gpt-5.6-terra".to_owned(),
            reasoning_effort: "medium".to_owned(),
            service_tier: "default".to_owned(),
            variant: String::new(),
            execution_harness: "builtin".to_owned(),
            permission_mode: "meeting_read_only".to_owned(),
            max_output_tokens: 0,
            catalog_revision: "catalog-1".to_owned(),
            persona_card_id: String::new(),
            runtime_profile_key: "profile-1".to_owned(),
            transport: "stdio_jsonl".to_owned(),
        }
    }
}
