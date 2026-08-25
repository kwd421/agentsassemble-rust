use std::time::Duration;

use agentsassemble_domain::AuthenticatedPrincipal;
use agentsassemble_persistence::{
    LiveRuntimeReconciliation, PersistenceError, ProviderTurnReconciliationCursor,
    RuntimeReconciliationCandidate, RuntimeReconciliationCursor, RuntimeReconciliationObservation,
    SqliteStore,
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
        recover_dynamic_observed_runtime, recover_startup_observed_runtime,
        release_checkpointed_absence, stale_candidate,
    },
};

const LIVE_OBSERVATION_TIMEOUT: Duration = Duration::from_secs(2);
const RECOVERY_SCAN_INTERVAL: Duration = Duration::from_secs(1);
const RECOVERY_OBSERVATION_CONCURRENCY: usize = 8;

#[cfg(test)]
pub(crate) static RUNTIME_RECONCILIATION_TEST_LOCK: tokio::sync::Mutex<()> =
    tokio::sync::Mutex::const_new(());

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
    let gone = matches!(&observation, ProviderRuntimeObservation::Gone);
    let reconciled = store
        .apply_live_runtime_reconciliation(&candidate, &persistence_observation(observation))
        .await?;
    if gone && reconciled == LiveRuntimeReconciliation::RetryOriginalEffect {
        release_checkpointed_absence(provider_adapter, &candidate).await;
    }
    Ok(reconciled)
}

pub(crate) async fn watch_runtime_reconciliation(
    store: SqliteStore,
    provider_adapter: ProviderAdapter,
    rooms: RoomRuntime,
    cancellation: CancellationToken,
) {
    let mut cursor: Option<RuntimeReconciliationCursor> = None;
    let mut provider_turn_cursor: Option<ProviderTurnReconciliationCursor> = None;
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
        let provider_page = tokio::select! {
            () = cancellation.cancelled() => return,
            page = store.load_provider_turn_reconciliation_page(provider_turn_cursor.as_ref()) => page,
        };
        match provider_page {
            Ok(page) => {
                provider_turn_cursor = page.next_cursor;
                crate::provider_turn_reconciliation_runtime::reconcile_live_page(
                    &store,
                    &provider_adapter,
                    &rooms,
                    page.candidates,
                    &cancellation,
                )
                .await;
            }
            Err(error) => {
                tracing::warn!(%error, "server-owned provider-turn recovery scan failed");
            }
        }
        let lifecycle_page = tokio::select! {
            () = cancellation.cancelled() => return,
            page = store.load_runtime_reconciliation_page(cursor.as_ref()) => page,
        };
        match lifecycle_page {
            Ok(page) => {
                cursor = page.next_cursor;
                reconcile_dynamic_candidates(
                    &store,
                    &provider_adapter,
                    &rooms,
                    page.candidates,
                    &cancellation,
                )
                .await;
            }
            Err(error) => {
                tracing::warn!(%error, "server-owned lifecycle recovery scan failed");
            }
        }
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

pub(crate) async fn checkpoint_confirmed_shutdown(
    store: &SqliteStore,
    stopped: &ProviderRuntimeGone,
) -> Result<(), PersistenceError> {
    if let Some(candidate) = store
        .load_active_provider_turn_reconciliation_candidate(&stopped.room_id, &stopped.session_id)
        .await?
    {
        let execution = &candidate.execution;
        if execution.runtime_handle_id != stopped.runtime_handle_id
            || execution.runtime_owner_id != stopped.runtime_owner_id
            || execution.runtime_lease_token != stopped.runtime_lease_token
        {
            return Err(stale_shutdown_observation());
        }
        store
            .finalize_provider_turn_runtime_gone(&candidate)
            .await?;
        return Ok(());
    }
    let Some(candidate) = store
        .load_runtime_reconciliation_candidate(&stopped.room_id, &stopped.session_id)
        .await?
    else {
        return Err(stale_shutdown_observation());
    };
    if candidate.session.runtime_handle_id != stopped.runtime_handle_id
        || candidate.session.runtime_owner_id != stopped.runtime_owner_id
        || candidate.session.runtime_lease_token != stopped.runtime_lease_token
    {
        return Err(stale_shutdown_observation());
    }
    store
        .apply_runtime_reconciliation(&candidate, &RuntimeReconciliationObservation::Gone)
        .await?;
    Ok(())
}

fn stale_shutdown_observation() -> PersistenceError {
    PersistenceError::CommandRejected {
        code: "stale_reconciliation_candidate",
        message: "Confirmed shutdown no longer matches durable runtime authority.".to_owned(),
    }
}

#[cfg(test)]
pub(crate) mod tests {
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

    use super::{RUNTIME_RECONCILIATION_TEST_LOCK, recover_exact_lifecycle_command};
    use crate::runtime_reconciliation_cleanup::commit_dynamic_gone;

    #[tokio::test]
    async fn exact_replay_releases_a_safe_failure_tombstone_after_db_recovery() {
        let _serial = RUNTIME_RECONCILIATION_TEST_LOCK.lock().await;
        let directory =
            tempfile::tempdir().unwrap_or_else(|error| panic!("create fixture: {error}"));
        let store = dynamic_recovery_store(directory.path()).await;
        let principal = local_principal();
        let mut failed_draft = draft(
            directory.path(),
            "codex-00000000-0000-5000-8000-000000000204",
        );
        failed_draft.provider_kind = "unsupported_test_provider".to_owned();
        failed_draft.transport = "unsupported_test_transport".to_owned();
        failed_draft.runtime_profile_key = draft_profile_key(&failed_draft);
        let created = store
            .execute_agent_create(
                &principal,
                "create-safe-failure-agent",
                &json!({"provider_id": "unsupported_test_provider"}),
                &failed_draft,
            )
            .await
            .unwrap_or_else(|error| panic!("create safe failure agent: {error}"));
        let session_id = created.result["agent_session"]["session_id"]
            .as_str()
            .unwrap_or_else(|| panic!("created session has no id"));
        let payload = json!({"agent_id": session_id});
        let AgentStartPlan::Start(effect) = store
            .prepare_agent_start(&principal, "safe-failure-replay", &payload)
            .await
            .unwrap_or_else(|error| panic!("prepare safe failure start: {error}"))
        else {
            panic!("stopped session must prepare a start effect");
        };
        let provider_adapter = ProviderAdapter::new();
        let reservation = provider_adapter
            .reserve_start(&effect.session)
            .await
            .unwrap_or_else(|error| panic!("reserve failed generation: {error}"));
        let authorized = store
            .authorize_agent_start_effect(
                &principal,
                "safe-failure-replay",
                &payload,
                &effect.operation_id,
                "agent.start",
                &reservation.runtime_handle_id,
                &reservation.runtime_owner_id,
                &reservation.runtime_lease_token,
            )
            .await
            .unwrap_or_else(|error| panic!("authorize failed generation: {error}"));
        let Err(failure) = provider_adapter.start_reserved(&authorized.session).await else {
            panic!("unsupported provider tuple must fail safely");
        };
        assert!(failure.runtime_stopped);

        assert_eq!(
            recover_exact_lifecycle_command(
                &store,
                &provider_adapter,
                &principal,
                "safe-failure-replay",
                "agent.start",
                &payload,
            )
            .await
            .unwrap_or_else(|error| panic!("recover safe failure: {error}")),
            LiveRuntimeReconciliation::RetryOriginalEffect
        );
        let AgentStartPlan::Start(retry) = store
            .prepare_agent_start(&principal, "safe-failure-replay", &payload)
            .await
            .unwrap_or_else(|error| panic!("reload recovered start: {error}"))
        else {
            panic!("recovered request must own its original start effect");
        };
        let next = provider_adapter
            .reserve_start(&retry.session)
            .await
            .unwrap_or_else(|error| panic!("reserve fresh generation after recovery: {error}"));
        assert_ne!(next.runtime_lease_token, reservation.runtime_lease_token);
        provider_adapter
            .cancel_start_reservation("general", session_id, &next)
            .await;
    }

    #[tokio::test]
    async fn dynamic_scan_discovers_later_intent_and_drops_stale_live_recovery_candidate() {
        let _serial = RUNTIME_RECONCILIATION_TEST_LOCK.lock().await;
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
                &reservation.runtime_lease_token,
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

    pub(super) async fn dynamic_recovery_store(root: &Path) -> SqliteStore {
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

    pub(crate) fn local_principal() -> AuthenticatedPrincipal {
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

    pub(crate) fn draft(workspace: &Path, agent_id: &str) -> AgentSessionDraft {
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

    pub(super) fn draft_profile_key(draft: &AgentSessionDraft) -> String {
        format!(
            "provider-profile-v1-{:x}",
            Sha256::digest(
                [
                    draft.provider_kind.as_str(),
                    draft.runtime_kind.as_str(),
                    draft.executable.as_str(),
                    draft.executable_identity.as_str(),
                    draft.workspace.as_str(),
                    draft.workspace_identity.as_str(),
                    draft.model.as_str(),
                    draft.reasoning_effort.as_str(),
                    draft.service_tier.as_str(),
                    draft.variant.as_str(),
                    draft.execution_harness.as_str(),
                    draft.permission_mode.as_str(),
                    draft.transport.as_str(),
                ]
                .join("\0")
                .as_bytes(),
            )
        )
    }
}

#[cfg(test)]
#[path = "runtime_reconciliation_owner_loss_tests.rs"]
mod owner_loss_tests;

#[cfg(test)]
#[path = "runtime_reconciliation_release_tests.rs"]
mod release_tests;
