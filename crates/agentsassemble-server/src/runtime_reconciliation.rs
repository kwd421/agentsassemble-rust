use std::time::Duration;

use agentsassemble_domain::AuthenticatedPrincipal;
use agentsassemble_persistence::{
    LiveRuntimeReconciliation, PersistenceError, RuntimeReconciliationObservation, SqliteStore,
};
use agentsassemble_provider::{ProviderAdapter, ProviderRuntimeGone, ProviderRuntimeObservation};
use futures_util::future::join_all;
use serde_json::Value;
use tokio::time::MissedTickBehavior;
use tokio_util::sync::CancellationToken;

use crate::RoomRuntime;

const LIVE_OBSERVATION_TIMEOUT: Duration = Duration::from_secs(2);
const STARTUP_RECOVERY_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StartupRecoveryKey {
    room_id: String,
    session_id: String,
}

/// Reconciles every durable runtime candidate before network admission.
///
/// # Errors
///
/// Returns a persistence error if an exact observed transition cannot commit.
pub async fn reconcile_runtime_ownership(
    store: &SqliteStore,
    provider_adapter: &ProviderAdapter,
) -> Result<usize, PersistenceError> {
    let candidates = store.load_runtime_reconciliation_candidates().await?;
    let observations = join_all(
        candidates
            .iter()
            .map(|candidate| provider_adapter.observe(&candidate.session)),
    )
    .await;
    for (candidate, observation) in candidates.iter().zip(observations) {
        let observation = persistence_observation(observation);
        store
            .apply_runtime_reconciliation(candidate, &observation)
            .await?;
    }
    Ok(candidates.len())
}

pub(crate) async fn recover_exact_lifecycle_command(
    store: &SqliteStore,
    provider_adapter: &ProviderAdapter,
    principal: &AuthenticatedPrincipal,
    request_id: &str,
    action: &str,
    payload: &Value,
) -> Result<LiveRuntimeReconciliation, PersistenceError> {
    let candidate = store
        .load_lifecycle_reconciliation_candidate(principal, request_id, action, payload)
        .await?;
    let Ok(observation) = tokio::time::timeout(
        LIVE_OBSERVATION_TIMEOUT,
        provider_adapter.observe(&candidate.session),
    )
    .await
    else {
        return Ok(LiveRuntimeReconciliation::StillUnresolved);
    };
    store
        .apply_live_runtime_reconciliation(&candidate, &persistence_observation(observation))
        .await
}

pub(crate) async fn startup_recovery_keys(
    store: &SqliteStore,
) -> Result<Vec<StartupRecoveryKey>, PersistenceError> {
    Ok(store
        .load_runtime_reconciliation_candidates()
        .await?
        .into_iter()
        .filter(|candidate| candidate.reservation.is_some())
        .map(|candidate| StartupRecoveryKey {
            room_id: candidate.session.public.room_id,
            session_id: candidate.session.public.session_id,
        })
        .collect())
}

pub(crate) async fn watch_startup_recovery(
    store: SqliteStore,
    provider_adapter: ProviderAdapter,
    rooms: RoomRuntime,
    cancellation: CancellationToken,
    mut pending: Vec<StartupRecoveryKey>,
) {
    if pending.is_empty() {
        return;
    }
    let mut interval = tokio::time::interval(STARTUP_RECOVERY_INTERVAL);
    interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            () = cancellation.cancelled() => return,
            _ = interval.tick() => {}
        }
        let results = join_all(
            pending
                .iter()
                .map(|key| reconcile_startup_key(&store, &provider_adapter, &rooms, key)),
        )
        .await;
        let mut retained = Vec::new();
        for (key, result) in pending.into_iter().zip(results) {
            match result {
                Ok(true) => retained.push(key),
                Ok(false) => {}
                Err(_) => {
                    tracing::warn!(
                        room_id = %key.room_id,
                        session_id = %key.session_id,
                        "server-owned lifecycle recovery observation could not commit"
                    );
                    retained.push(key);
                }
            }
        }
        if retained.is_empty() {
            return;
        }
        pending = retained;
    }
}

async fn reconcile_startup_key(
    store: &SqliteStore,
    provider_adapter: &ProviderAdapter,
    rooms: &RoomRuntime,
    key: &StartupRecoveryKey,
) -> Result<bool, PersistenceError> {
    let Some(candidate) = store
        .load_runtime_reconciliation_candidate(&key.room_id, &key.session_id)
        .await?
    else {
        return Ok(false);
    };
    if candidate.reservation.is_none() {
        return Ok(false);
    }
    let Ok(observation) = tokio::time::timeout(
        LIVE_OBSERVATION_TIMEOUT,
        provider_adapter.observe(&candidate.session),
    )
    .await
    else {
        return Ok(true);
    };
    if observation != ProviderRuntimeObservation::Gone {
        return Ok(true);
    }
    store
        .apply_runtime_reconciliation(&candidate, &RuntimeReconciliationObservation::Gone)
        .await?;
    rooms.notify_room_publication(&key.room_id).await;
    Ok(false)
}

pub(crate) fn persistence_observation(
    observation: ProviderRuntimeObservation,
) -> RuntimeReconciliationObservation {
    match observation {
        ProviderRuntimeObservation::Adopted {
            handle_id,
            previous_owner_id,
            new_owner_id,
            runtime_profile_key,
        } => RuntimeReconciliationObservation::Adopted {
            handle_id,
            previous_owner_id,
            new_owner_id,
            runtime_profile_key,
        },
        ProviderRuntimeObservation::Gone => RuntimeReconciliationObservation::Gone,
        ProviderRuntimeObservation::LeaseUncertain {
            handle_id,
            owner_id,
            reason_code,
        } => RuntimeReconciliationObservation::LeaseUncertain {
            handle_id,
            owner_id,
            reason_code,
        },
        ProviderRuntimeObservation::Ambiguous { reason_code } => {
            RuntimeReconciliationObservation::Ambiguous { reason_code }
        }
    }
}

pub(crate) async fn checkpoint_confirmed_shutdowns(
    store: &SqliteStore,
    gone: &[ProviderRuntimeGone],
) -> Result<(), PersistenceError> {
    if gone.is_empty() {
        return Ok(());
    }
    let candidates = store.load_runtime_reconciliation_candidates().await?;
    for stopped in gone {
        let Some(candidate) = candidates.iter().find(|candidate| {
            candidate.session.public.room_id == stopped.room_id
                && candidate.session.public.session_id == stopped.session_id
        }) else {
            continue;
        };
        let durable_identity_is_empty = candidate.session.runtime_handle_id.is_empty()
            && candidate.session.runtime_owner_id.is_empty();
        let durable_identity_matches = candidate.session.runtime_handle_id
            == stopped.runtime_handle_id
            && candidate.session.runtime_owner_id == stopped.runtime_owner_id;
        if !durable_identity_is_empty && !durable_identity_matches {
            return Err(PersistenceError::CommandRejected {
                code: "stale_reconciliation_candidate",
                message: "Confirmed shutdown no longer matches durable runtime authority."
                    .to_owned(),
            });
        }
        store
            .apply_runtime_reconciliation(candidate, &RuntimeReconciliationObservation::Gone)
            .await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{fs::File, path::Path};

    use agentsassemble_domain::{
        AgentSessionDraft, AuthenticatedPrincipal, CapabilitySet, ClientKind, InviteScope,
        LOCAL_OPERATOR_PARTICIPANT_ID, stable_content_identity, stable_identity_hash,
    };
    use agentsassemble_persistence::{AgentStartPlan, LiveRuntimeReconciliation, SqliteStore};
    use agentsassemble_provider::ProviderAdapter;
    use same_file::Handle;
    use serde_json::json;

    use super::recover_exact_lifecycle_command;

    #[tokio::test]
    async fn production_replay_helper_observes_gone_before_reenabling_start() {
        let directory =
            tempfile::tempdir().unwrap_or_else(|error| panic!("create fixture: {error}"));
        let store = SqliteStore::open_path(&directory.path().join("runtime.sqlite3"))
            .await
            .unwrap_or_else(|error| panic!("open store: {error}"));
        store
            .bootstrap_local_authority("16193216-8799-4f67-ad17-f05c7da0f433", "Host")
            .await
            .unwrap_or_else(|error| panic!("bootstrap identity: {error}"));
        store
            .create_room_for_local_operator(
                "67e86a68-c52b-4ffc-8039-c908a33a9150",
                "general",
                "General",
            )
            .await
            .unwrap_or_else(|error| panic!("create room: {error}"));
        let principal = local_principal();
        let created = store
            .execute_agent_create(
                &principal,
                "create-recovery-agent",
                &json!({"provider_id": "codex"}),
                &draft(directory.path()),
            )
            .await
            .unwrap_or_else(|error| panic!("create agent: {error}"));
        let session_id = created.result["agent_session"]["session_id"]
            .as_str()
            .unwrap_or_else(|| panic!("created session has no id"));
        let payload = json!({"agent_id": session_id});
        let AgentStartPlan::Start(effect) = store
            .prepare_agent_start(&principal, "live-helper-start", &payload)
            .await
            .unwrap_or_else(|error| panic!("prepare start: {error}"))
        else {
            panic!("stopped session must prepare a start effect");
        };
        store
            .mark_agent_start_unconfirmed(
                &principal,
                session_id,
                &effect.operation_id,
                "",
                "",
                "runtime_start_unconfirmed",
                "provider effect boundary was uncertain",
            )
            .await
            .unwrap_or_else(|error| panic!("mark start unconfirmed: {error}"));

        assert_eq!(
            recover_exact_lifecycle_command(
                &store,
                &ProviderAdapter::new(),
                &principal,
                "live-helper-start",
                "agent.start",
                &payload,
            )
            .await
            .unwrap_or_else(|error| panic!("recover exact start: {error}")),
            LiveRuntimeReconciliation::RetryOriginalEffect
        );
        assert!(matches!(
            store
                .prepare_agent_start(&principal, "live-helper-start", &payload)
                .await
                .unwrap_or_else(|error| panic!("re-enter start: {error}")),
            AgentStartPlan::Start(_)
        ));
    }

    fn local_principal() -> AuthenticatedPrincipal {
        AuthenticatedPrincipal {
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
        }
    }

    fn draft(workspace: &Path) -> AgentSessionDraft {
        let executable = std::env::current_exe()
            .and_then(std::fs::canonicalize)
            .unwrap_or_else(|error| panic!("canonical executable: {error}"));
        let mut file =
            File::open(&executable).unwrap_or_else(|error| panic!("open executable: {error}"));
        let executable_handle = Handle::from_file(
            file.try_clone()
                .unwrap_or_else(|error| panic!("clone executable: {error}")),
        )
        .unwrap_or_else(|error| panic!("identify executable: {error}"));
        let workspace = workspace
            .canonicalize()
            .unwrap_or_else(|error| panic!("canonical workspace: {error}"));
        AgentSessionDraft {
            agent_id: "codex-00000000-0000-5000-8000-000000000201".to_owned(),
            display_name: "Terra".to_owned(),
            provider_kind: "codex_app_server".to_owned(),
            runtime_kind: "live_cli".to_owned(),
            executable: executable.to_string_lossy().into_owned(),
            executable_identity: stable_content_identity(&executable_handle, &mut file)
                .unwrap_or_else(|error| panic!("hash executable: {error}")),
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
            catalog_revision: "catalog-recovery-1".to_owned(),
            runtime_profile_key: "profile-recovery-1".to_owned(),
            transport: "stdio_jsonl".to_owned(),
        }
    }
}
