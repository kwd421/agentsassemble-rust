use std::time::Duration;

use agentsassemble_domain::AuthenticatedPrincipal;
use agentsassemble_persistence::{
    LiveRuntimeReconciliation, PersistenceError, RuntimeReconciliationCandidate,
    RuntimeReconciliationCursor, RuntimeReconciliationObservation, SqliteStore,
};
use agentsassemble_provider::{ProviderAdapter, ProviderRuntimeGone, ProviderRuntimeObservation};
use futures_util::{StreamExt, stream};
use serde_json::Value;
use tokio::time::{Instant, MissedTickBehavior};
use tokio_util::sync::CancellationToken;

use crate::{
    RoomRuntime,
    runtime_reconciliation_cleanup::{
        commit_and_publish_gone, commit_startup_gone, persistence_observation,
        recover_dynamic_observed_runtime, recover_startup_observed_runtime, stale_candidate,
    },
};

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
    let mut observed_candidates = Vec::new();
    for candidate in candidates {
        match candidate.session.lifecycle_intent_status.as_str() {
            "prepared" => {
                store
                    .reject_abandoned_lifecycle_before_effect(&candidate)
                    .await?;
            }
            "effect_applied" => {
                commit_startup_gone(store, provider_adapter, &candidate).await?;
            }
            _ => observed_candidates.push(candidate),
        }
    }
    let observations = stream::iter(observed_candidates)
        .map(|candidate| async move {
            let observation = provider_adapter.observe(&candidate.session).await;
            (candidate, observation)
        })
        .buffer_unordered(RECOVERY_OBSERVATION_CONCURRENCY)
        .collect::<Vec<_>>()
        .await;
    for (candidate, observation) in observations {
        match observation {
            observation @ (ProviderRuntimeObservation::Adopted { .. }
            | ProviderRuntimeObservation::LeaseUncertain { .. }) => {
                recover_startup_observed_runtime(store, provider_adapter, &candidate, observation)
                    .await?;
            }
            ProviderRuntimeObservation::Gone => {
                commit_startup_gone(store, provider_adapter, &candidate).await?;
            }
            observation @ ProviderRuntimeObservation::Ambiguous { .. } => {
                store
                    .apply_runtime_reconciliation(&candidate, &persistence_observation(observation))
                    .await?;
            }
        }
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
    let mut interval = tokio::time::interval_at(
        Instant::now() + RECOVERY_SCAN_INTERVAL,
        RECOVERY_SCAN_INTERVAL,
    );
    interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            () = cancellation.cancelled() => return,
            _ = interval.tick() => {}
        }
        let page = match tokio::select! {
            () = cancellation.cancelled() => return,
            page = store.load_runtime_reconciliation_page(cursor.as_ref()) => page,
        } {
            Ok(page) => page,
            Err(error) => {
                tracing::warn!(%error, "server-owned lifecycle recovery scan failed");
                continue;
            }
        };
        cursor = page.next_cursor;
        reconcile_dynamic_candidates(
            &store,
            &provider_adapter,
            &rooms,
            page.candidates,
            &cancellation,
        )
        .await;
        if cancellation.is_cancelled() {
            return;
        }
    }
}

async fn reconcile_dynamic_candidates(
    store: &SqliteStore,
    provider_adapter: &ProviderAdapter,
    rooms: &RoomRuntime,
    candidates: Vec<RuntimeReconciliationCandidate>,
    cancellation: &CancellationToken,
) {
    let mut needs_observation = Vec::new();
    for candidate in candidates {
        if cancellation.is_cancelled() {
            return;
        }
        let Some(command_owner) = claim_candidate(rooms, &candidate) else {
            continue;
        };
        match candidate.session.lifecycle_intent_status.as_str() {
            "prepared" => {
                commit_abandoned_pre_effect(store, rooms, &candidate).await;
            }
            "effect_applied" => {
                commit_and_publish_gone(store, provider_adapter, rooms, &candidate).await;
            }
            "effect_inflight" | "unconfirmed" => {
                needs_observation.push((candidate, command_owner));
            }
            _ => {}
        }
    }
    let observed = stream::iter(needs_observation)
        .map(|(candidate, command_owner)| async move {
            let observation = tokio::time::timeout(
                LIVE_OBSERVATION_TIMEOUT,
                provider_adapter.observe(&candidate.session),
            )
            .await
            .ok();
            (candidate, command_owner, observation)
        })
        .buffer_unordered(RECOVERY_OBSERVATION_CONCURRENCY)
        .collect::<Vec<_>>()
        .await;
    for (candidate, _command_owner, observation) in observed {
        if cancellation.is_cancelled() {
            continue;
        }
        match observation {
            Some(ProviderRuntimeObservation::Gone) => {
                commit_and_publish_gone(store, provider_adapter, rooms, &candidate).await;
            }
            Some(
                observation @ (ProviderRuntimeObservation::Adopted { .. }
                | ProviderRuntimeObservation::LeaseUncertain { .. }),
            ) => {
                recover_dynamic_observed_runtime(
                    store,
                    provider_adapter,
                    rooms,
                    &candidate,
                    observation,
                )
                .await;
            }
            Some(ProviderRuntimeObservation::Ambiguous { .. }) | None => {}
        }
    }
}

fn claim_candidate(
    rooms: &RoomRuntime,
    candidate: &RuntimeReconciliationCandidate,
) -> Option<crate::lifecycle_command_tracker::LifecycleCommandGuard> {
    let reservation = candidate.reservation.as_ref()?;
    rooms.try_claim_lifecycle_command(
        &reservation.principal.room_id,
        &reservation.principal.principal_id,
        &reservation.request_id,
        &reservation.action,
    )
}

async fn commit_abandoned_pre_effect(
    store: &SqliteStore,
    rooms: &RoomRuntime,
    candidate: &RuntimeReconciliationCandidate,
) {
    let room_id = candidate.session.public.room_id.clone();
    match store
        .reject_abandoned_lifecycle_before_effect(candidate)
        .await
    {
        Ok(()) => rooms.notify_room_publication(&room_id).await,
        Err(error) if stale_candidate(&error) => {}
        Err(error) => tracing::warn!(
            %error,
            %room_id,
            session_id = %candidate.session.public.session_id,
            "server-owned pre-effect lifecycle recovery could not commit"
        ),
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
    use sha2::{Digest, Sha256};

    use super::recover_exact_lifecycle_command;
    use crate::runtime_reconciliation_cleanup::commit_dynamic_gone;

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
                &draft(
                    directory.path(),
                    "codex-00000000-0000-5000-8000-000000000201",
                ),
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
        let provider_adapter = ProviderAdapter::new();
        let reservation = provider_adapter
            .reserve_start(&effect.session)
            .await
            .unwrap_or_else(|error| panic!("reserve provider start: {error}"));
        store
            .authorize_agent_start_effect(
                &principal,
                "live-helper-start",
                &payload,
                &effect.operation_id,
                "agent.start",
                &reservation.runtime_handle_id,
                &reservation.runtime_owner_id,
            )
            .await
            .unwrap_or_else(|error| panic!("authorize provider start: {error}"));
        store
            .mark_agent_start_unconfirmed(
                &principal,
                session_id,
                &effect.operation_id,
                &reservation.runtime_handle_id,
                &reservation.runtime_owner_id,
                "runtime_start_unconfirmed",
                "provider effect boundary was uncertain",
            )
            .await
            .unwrap_or_else(|error| panic!("mark start unconfirmed: {error}"));

        assert_eq!(
            recover_exact_lifecycle_command(
                &store,
                &provider_adapter,
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
        let store = dynamic_recovery_store(directory.path()).await;
        assert!(
            store
                .load_runtime_reconciliation_page(None)
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
                &draft(
                    directory.path(),
                    "codex-00000000-0000-5000-8000-000000000202",
                ),
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
        let provider_adapter = ProviderAdapter::new();
        let reservation = provider_adapter
            .reserve_start(&effect.session)
            .await
            .unwrap_or_else(|error| panic!("reserve provider start: {error}"));
        store
            .authorize_agent_start_effect(
                &principal,
                "dynamic-start",
                &payload,
                &effect.operation_id,
                "agent.start",
                &reservation.runtime_handle_id,
                &reservation.runtime_owner_id,
            )
            .await
            .unwrap_or_else(|error| panic!("authorize provider start: {error}"));
        store
            .mark_agent_start_unconfirmed(
                &principal,
                session_id,
                &effect.operation_id,
                &reservation.runtime_handle_id,
                &reservation.runtime_owner_id,
                "runtime_start_unconfirmed",
                "provider effect boundary was uncertain",
            )
            .await
            .unwrap_or_else(|error| panic!("mark start unconfirmed: {error}"));
        let mut page = store
            .load_runtime_reconciliation_page(None)
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
        provider_adapter
            .cancel_start_reservation("general", session_id, &reservation)
            .await;
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

    async fn dynamic_recovery_store(root: &Path) -> SqliteStore {
        let store = SqliteStore::open_path(&root.join("runtime.sqlite3"))
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
        store
    }

    pub(super) fn local_principal() -> AuthenticatedPrincipal {
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

    pub(super) fn draft(workspace: &Path, agent_id: &str) -> AgentSessionDraft {
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
        let provider_kind = "opencode_server";
        let runtime_kind = "live_cli";
        let executable = executable.to_string_lossy().into_owned();
        let executable_identity = stable_content_identity(&executable_handle, &mut file)
            .unwrap_or_else(|error| panic!("hash executable: {error}"));
        let workspace = workspace.to_string_lossy().into_owned();
        let workspace_identity = stable_identity_hash(
            &Handle::from_path(&workspace)
                .unwrap_or_else(|error| panic!("identify workspace: {error}")),
        );
        let model = "gpt-5.6-terra";
        let reasoning_effort = "medium";
        let service_tier = "default";
        let variant = "";
        let execution_harness = "builtin";
        let permission_mode = "meeting_read_only";
        let transport = "http";
        let runtime_profile_key = format!(
            "provider-profile-v1-{:x}",
            Sha256::digest(
                [
                    provider_kind,
                    runtime_kind,
                    executable.as_str(),
                    executable_identity.as_str(),
                    workspace.as_str(),
                    workspace_identity.as_str(),
                    model,
                    reasoning_effort,
                    service_tier,
                    variant,
                    execution_harness,
                    permission_mode,
                    transport,
                ]
                .join("\0")
                .as_bytes(),
            )
        );
        AgentSessionDraft {
            agent_id: agent_id.to_owned(),
            display_name: "Terra".to_owned(),
            provider_kind: provider_kind.to_owned(),
            runtime_kind: runtime_kind.to_owned(),
            executable,
            executable_identity,
            workspace,
            workspace_identity,
            model: model.to_owned(),
            reasoning_effort: reasoning_effort.to_owned(),
            service_tier: service_tier.to_owned(),
            variant: variant.to_owned(),
            execution_harness: execution_harness.to_owned(),
            permission_mode: permission_mode.to_owned(),
            max_output_tokens: 0,
            catalog_revision: "catalog-recovery-1".to_owned(),
            runtime_profile_key,
            transport: transport.to_owned(),
        }
    }
}

#[cfg(test)]
#[path = "runtime_reconciliation_owner_loss_tests.rs"]
mod owner_loss_tests;
