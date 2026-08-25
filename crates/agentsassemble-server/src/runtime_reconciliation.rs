use std::time::Duration;

use agentsassemble_domain::AuthenticatedPrincipal;
use agentsassemble_persistence::{
    LiveRuntimeReconciliation, PersistenceError, RuntimeReconciliationCandidate,
    RuntimeReconciliationCursor, RuntimeReconciliationObservation, SqliteStore,
};
use agentsassemble_provider::{ProviderAdapter, ProviderRuntimeGone, ProviderRuntimeObservation};
use futures_util::{StreamExt, stream};
use serde_json::Value;
use tokio::time::MissedTickBehavior;
use tokio_util::sync::CancellationToken;

use crate::RoomRuntime;

const LIVE_OBSERVATION_TIMEOUT: Duration = Duration::from_secs(2);
const RECOVERY_SCAN_INTERVAL: Duration = Duration::from_secs(1);
const RECOVERY_OBSERVATION_CONCURRENCY: usize = 8;

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
    let candidate_count = candidates.len();
    let observations = stream::iter(candidates)
        .map(|candidate| async move {
            let observation = provider_adapter.observe(&candidate.session).await;
            (candidate, observation)
        })
        .buffer_unordered(RECOVERY_OBSERVATION_CONCURRENCY)
        .collect::<Vec<_>>()
        .await;
    for (candidate, observation) in observations {
        store
            .apply_runtime_reconciliation(&candidate, &persistence_observation(observation))
            .await?;
    }
    Ok(candidate_count)
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

pub(crate) async fn watch_runtime_reconciliation(
    store: SqliteStore,
    provider_adapter: ProviderAdapter,
    rooms: RoomRuntime,
    cancellation: CancellationToken,
) {
    let mut cursor: Option<RuntimeReconciliationCursor> = None;
    let mut interval = tokio::time::interval(RECOVERY_SCAN_INTERVAL);
    interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            () = cancellation.cancelled() => return,
            _ = interval.tick() => {}
        }
        let page = match store
            .load_unconfirmed_runtime_reconciliation_page(cursor.as_ref())
            .await
        {
            Ok(page) => page,
            Err(error) => {
                tracing::warn!(%error, "server-owned lifecycle recovery scan failed");
                continue;
            }
        };
        cursor = page.next_cursor;
        reconcile_dynamic_candidates(&store, &provider_adapter, &rooms, page.candidates).await;
    }
}

async fn reconcile_dynamic_candidates(
    store: &SqliteStore,
    provider_adapter: &ProviderAdapter,
    rooms: &RoomRuntime,
    candidates: Vec<RuntimeReconciliationCandidate>,
) {
    let observed = stream::iter(candidates)
        .map(|candidate| async move {
            let observation = tokio::time::timeout(
                LIVE_OBSERVATION_TIMEOUT,
                provider_adapter.observe(&candidate.session),
            )
            .await
            .ok();
            (candidate, observation)
        })
        .buffer_unordered(RECOVERY_OBSERVATION_CONCURRENCY)
        .collect::<Vec<_>>()
        .await;
    for (candidate, observation) in observed {
        if observation != Some(ProviderRuntimeObservation::Gone) {
            continue;
        }
        let room_id = candidate.session.public.room_id.clone();
        match commit_dynamic_gone(store, &candidate).await {
            Ok(true) => rooms.notify_room_publication(&room_id).await,
            Ok(false) => {}
            Err(error) => tracing::warn!(
                %error,
                %room_id,
                session_id = %candidate.session.public.session_id,
                "server-owned lifecycle recovery observation could not commit"
            ),
        }
    }
}

async fn commit_dynamic_gone(
    store: &SqliteStore,
    candidate: &RuntimeReconciliationCandidate,
) -> Result<bool, PersistenceError> {
    match store
        .apply_runtime_reconciliation(candidate, &RuntimeReconciliationObservation::Gone)
        .await
    {
        Ok(()) => Ok(true),
        Err(error) if stale_candidate(&error) => Ok(false),
        Err(error) => Err(error),
    }
}

fn stale_candidate(error: &PersistenceError) -> bool {
    matches!(
        error,
        PersistenceError::CommandRejected {
            code: "stale_reconciliation_candidate",
            ..
        }
    )
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
    use agentsassemble_persistence::{
        AgentStartPlan, LiveRuntimeReconciliation, RuntimeReconciliationObservation, SqliteStore,
    };
    use agentsassemble_provider::ProviderAdapter;
    use same_file::Handle;
    use serde_json::json;

    use super::{commit_dynamic_gone, recover_exact_lifecycle_command};

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

    #[tokio::test]
    async fn dynamic_scan_discovers_later_intent_and_drops_stale_live_recovery_candidate() {
        let directory =
            tempfile::tempdir().unwrap_or_else(|error| panic!("create fixture: {error}"));
        let store = SqliteStore::open_path(&directory.path().join("runtime.sqlite3"))
            .await
            .unwrap_or_else(|error| panic!("open store: {error}"));
        store
            .bootstrap_local_authority("26193216-8799-4f67-ad17-f05c7da0f433", "Host")
            .await
            .unwrap_or_else(|error| panic!("bootstrap identity: {error}"));
        store
            .create_room_for_local_operator(
                "77e86a68-c52b-4ffc-8039-c908a33a9150",
                "general",
                "General",
            )
            .await
            .unwrap_or_else(|error| panic!("create room: {error}"));
        assert!(
            store
                .load_unconfirmed_runtime_reconciliation_page(None)
                .await
                .unwrap_or_else(|error| panic!("scan empty store: {error}"))
                .candidates
                .is_empty()
        );

        let principal = local_principal();
        let created = store
            .execute_agent_create(
                &principal,
                "create-dynamic-agent",
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
            .prepare_agent_start(&principal, "dynamic-start", &payload)
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
        let mut page = store
            .load_unconfirmed_runtime_reconciliation_page(None)
            .await
            .unwrap_or_else(|error| panic!("scan later intent: {error}"));
        assert_eq!(page.candidates.len(), 1);
        let candidate = page.candidates.pop().unwrap_or_else(|| panic!("candidate"));

        assert_eq!(
            store
                .apply_live_runtime_reconciliation(
                    &candidate,
                    &RuntimeReconciliationObservation::Gone,
                )
                .await
                .unwrap_or_else(|error| panic!("apply live recovery: {error}")),
            LiveRuntimeReconciliation::RetryOriginalEffect
        );
        assert!(
            !commit_dynamic_gone(&store, &candidate)
                .await
                .unwrap_or_else(|error| panic!("drop stale watcher candidate: {error}"))
        );
        assert!(matches!(
            store
                .prepare_agent_start(&principal, "dynamic-start", &payload)
                .await
                .unwrap_or_else(|error| panic!("re-enter recovered start: {error}")),
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
