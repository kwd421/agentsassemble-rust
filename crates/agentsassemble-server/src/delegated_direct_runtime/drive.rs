//! Approved delegated execution drives: catalog re-validation for bound
//! create/reconfigure requests and the post-decision confirmed drives.
use agentsassemble_domain::{AgentSessionDraft, ConfirmationState};
use agentsassemble_persistence::{
    ApprovedCreateDrive, ApprovedReconfigureDrive, PersistenceError, SqliteStore,
    create_execution_payload, create_selection_payload, reconfigure_execution_payload,
    reconfigure_selection_payload,
};
use agentsassemble_provider::ProviderCatalogService;
use serde_json::Value;

use crate::lifecycle_command_tracker::LifecycleCommandTracker;
use crate::room_agent_lifecycle_runtime::{LifecycleCommand, execute_agent_start};
use crate::room_command_execution::CommandExecution;
use crate::room_runtime::RoomCommand;

/// Returns the driveable confirmation of one fresh approved room-command
/// decision: its reply must still carry the `executing` state for this
/// receipt action together with its confirmation ID.
fn approved_decision(
    command: &RoomCommand,
    execution: &CommandExecution,
    receipt_action: &str,
) -> Option<String> {
    if command.action != agentsassemble_protocol::RoomAction::DelegatedActionDecide {
        return None;
    }
    let Ok(outcome) = &execution.reply else {
        return None;
    };
    if outcome.deduplicated
        || outcome.result.get("state").and_then(Value::as_str) != Some("executing")
        || outcome
            .result
            .get("receipt")
            .and_then(|receipt| receipt.get("action"))
            .and_then(Value::as_str)
            != Some(receipt_action)
    {
        return None;
    }
    outcome
        .result
        .get("confirmation_id")
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn owner_principal(owner_reference: &Value) -> Result<&str, PersistenceError> {
    owner_reference
        .get("user_id")
        .and_then(Value::as_str)
        .ok_or_else(|| PersistenceError::CommandRejected {
            code: "owner_authority_unavailable".into(),
            message: "The bound owner's authority is unavailable.".into(),
        })
}

/// Re-runs the owner's catalog selection for a bound create request and drives
/// its approved execution. Selection drift refuses with `target_changed`;
/// execution itself is idempotent, so lost drives safely retry the same receipt.
pub(crate) async fn drive_approved_create(
    store: &SqliteStore,
    provider_catalog: &ProviderCatalogService,
    room_id: &str,
    confirmation_id: &str,
) -> Result<ApprovedCreateDrive, PersistenceError> {
    let (request, owner_reference, receipt) = store
        .approved_create_snapshot(room_id, confirmation_id)
        .await?;
    if request.state != ConfirmationState::Executing {
        return Ok(ApprovedCreateDrive {
            state: request.state,
            event_id: receipt
                .get("event_id")
                .and_then(Value::as_str)
                .map(str::to_owned),
            receipt,
            new_events: Vec::new(),
        });
    }
    let agentsassemble_domain::ConfirmedRoomAction::CreateAgent(create) = &request.action else {
        return Err(PersistenceError::CommandRejected {
            code: "bad_request".into(),
            message: "An agent creation is required.".into(),
        });
    };
    let owner_principal_id = owner_principal(&owner_reference)?;
    let selection = match provider_catalog
        .validate_creation(
            room_id,
            owner_principal_id,
            &create.request_id,
            &create_selection_payload(&create.params),
        )
        .await
    {
        Ok(selection) => selection,
        Err(error) => {
            return store
                .fail_approved_agent_create(
                    room_id,
                    confirmation_id,
                    "target_changed",
                    &error.message,
                )
                .await;
        }
    };
    let fresh: AgentSessionDraft = selection.into();
    if fresh != create.draft {
        return store
            .fail_approved_agent_create(
                room_id,
                confirmation_id,
                "target_changed",
                "The catalog selection changed before approval.",
            )
            .await;
    }
    store
        .drive_approved_agent_create(
            room_id,
            confirmation_id,
            &fresh,
            &create_execution_payload(&create.params, &fresh, &request),
            &owner_reference,
        )
        .await
}

/// Re-runs the owner's catalog selection and target binding for an approved
/// reconfiguration, then drives its configuration core. A moved configuration
/// revision, missing target or catalog drift refuses with `target_changed`;
/// execution itself is idempotent, so lost drives safely retry the same receipt.
pub(crate) async fn drive_approved_reconfigure(
    store: &SqliteStore,
    provider_catalog: &ProviderCatalogService,
    room_id: &str,
    confirmation_id: &str,
) -> Result<ApprovedReconfigureDrive, PersistenceError> {
    let (request, owner_reference, receipt) = store
        .approved_reconfigure_snapshot(room_id, confirmation_id)
        .await?;
    if request.state != ConfirmationState::Executing {
        return Ok(ApprovedReconfigureDrive {
            state: request.state,
            event_id: receipt
                .get("event_id")
                .and_then(Value::as_str)
                .map(str::to_owned),
            receipt,
            new_events: Vec::new(),
        });
    }
    let agentsassemble_domain::ConfirmedRoomAction::ReconfigureAgent(reconfigure) = &request.action
    else {
        return Err(PersistenceError::CommandRejected {
            code: "bad_request".into(),
            message: "An agent reconfiguration is required.".into(),
        });
    };
    let owner_principal_id = owner_principal(&owner_reference)?;
    let current = match store
        .delegated_reconfigure_session(room_id, &reconfigure.agent_id)
        .await
    {
        Ok(session) => session,
        Err(_) => {
            return store
                .fail_approved_agent_reconfigure(
                    room_id,
                    confirmation_id,
                    "target_changed",
                    "The target agent no longer exists.",
                )
                .await;
        }
    };
    if current.runtime_profile_key != reconfigure.expected_configuration_revision {
        return store
            .fail_approved_agent_reconfigure(
                room_id,
                confirmation_id,
                "target_changed",
                "The target agent configuration changed before approval.",
            )
            .await;
    }
    let selection = match provider_catalog
        .validate_configuration(
            room_id,
            owner_principal_id,
            &reconfigure.request_id,
            &current,
            &reconfigure_selection_payload(&reconfigure.params),
        )
        .await
    {
        Ok(selection) => selection,
        Err(error) => {
            return store
                .fail_approved_agent_reconfigure(
                    room_id,
                    confirmation_id,
                    "target_changed",
                    &error.message,
                )
                .await;
        }
    };
    let fresh: AgentSessionDraft = selection.into();
    if fresh != reconfigure.draft {
        return store
            .fail_approved_agent_reconfigure(
                room_id,
                confirmation_id,
                "target_changed",
                "The catalog selection changed before approval.",
            )
            .await;
    }
    store
        .drive_approved_agent_reconfigure(
            room_id,
            confirmation_id,
            &fresh,
            &reconfigure_execution_payload(&reconfigure.params, &request),
            &owner_reference,
        )
        .await
}

/// An approved confirmed reconfiguration reserved only its decision in the approval
/// transaction; re-resolve the catalog and drive the configuration core with its
/// attributed events.
pub(crate) async fn drive_approved_confirmed_reconfigure(
    store: &SqliteStore,
    provider_catalog: &ProviderCatalogService,
    command: &RoomCommand,
    execution: &mut CommandExecution,
) {
    let Some(confirmation_id) = approved_decision(command, execution, "reconfigure_agent") else {
        return;
    };
    let drive = drive_approved_reconfigure(
        store,
        provider_catalog,
        &command.principal.room_id,
        &confirmation_id,
    )
    .await;
    match drive {
        Ok(drive) => {
            execution.committed_events.extend(drive.new_events);
        }
        Err(error) => {
            // Uncertain effects stay executing; the AI retry re-drives the receipt.
            tracing::warn!(error = ?error, room_id = %command.principal.room_id, confirmation_id, "approved confirmed reconfiguration did not complete");
        }
    }
}

/// An approved confirmed creation reserved only its decision in the approval
/// transaction; drive the catalog-selected creation through the same core the
/// owner's own stopped create uses, then publish its attributed events.
pub(crate) async fn drive_approved_confirmed_create(
    store: &SqliteStore,
    provider_catalog: &ProviderCatalogService,
    command: &RoomCommand,
    execution: &mut CommandExecution,
) {
    let Some(confirmation_id) = approved_decision(command, execution, "create_agent") else {
        return;
    };
    let drive = drive_approved_create(
        store,
        provider_catalog,
        &command.principal.room_id,
        &confirmation_id,
    )
    .await;
    match drive {
        Ok(drive) => {
            execution.committed_events.extend(drive.new_events);
        }
        Err(error) => {
            // Uncertain effects stay executing; the AI retry re-drives the receipt.
            tracing::warn!(error = ?error, room_id = %command.principal.room_id, confirmation_id, "approved confirmed create did not complete");
        }
    }
}
/// An approved confirmed agent start reserved its lifecycle operation inside the
/// decision transaction; drive it through the same gate the owner's own start uses.
/// The owner's ACK still reports the durable in-progress receipt; completion,
/// failure and recovery travel through the existing lifecycle custody.
pub(crate) async fn drive_approved_confirmed_start(
    store: &SqliteStore,
    provider_adapter: &agentsassemble_provider::ProviderAdapter,
    lifecycle_commands: &LifecycleCommandTracker,
    command: &RoomCommand,
    execution: &mut CommandExecution,
) {
    let Some(confirmation_id) = approved_decision(command, execution, "start_agent") else {
        return;
    };
    let control = match store
        .confirmed_agent_start(&command.principal.room_id, &confirmation_id)
        .await
    {
        Ok(control) => control,
        Err(error) => {
            tracing::error!(error = ?error, room_id = %command.principal.room_id, "approved confirmed start could not load its reserved control");
            return;
        }
    };
    let Some(_guard) = lifecycle_commands.try_claim(
        &control.principal().room_id,
        &control.principal().principal_id,
        control.request_id(),
        "agent.start",
    ) else {
        // Server recovery owns the reserved command; it reconciles the effect.
        return;
    };
    let lifecycle = LifecycleCommand::delegated(
        &control,
        agentsassemble_protocol::RoomAction::AgentStart,
        None,
    );
    let driven = execute_agent_start(store, provider_adapter, &lifecycle).await;
    execution.committed_events.extend(driven.committed_events);
    execution.assignments.extend(driven.assignments);
    if let Err(failure) = driven.reply {
        // Terminal failures are already recorded in the confirmation receipt;
        // uncertain effects stay executing under lifecycle reconciliation.
        tracing::warn!(error = ?failure.error, room_id = %command.principal.room_id, confirmation_id, "approved confirmed start did not complete");
    }
}
