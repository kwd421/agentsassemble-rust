use agentsassemble_domain::{AgentSessionDraft, DirectRoomAction, canonical_payload_hash};
use agentsassemble_persistence::{
    DelegateRoomAuthority, PersistenceError, create_selection_payload,
    reconfigure_selection_payload,
};
use serde_json::{Value, json};
use tokio::sync::oneshot;

use super::{RoomCommandOwners, RoomMutation, RoomRuntime};
use crate::principal_mutation_admission::{MutationIdentity, PrincipalMutationAdmission};

mod apply;
mod drive;

pub(crate) use self::apply::apply;
use self::apply::{in_progress, publish};
pub(crate) use self::drive::{
    drive_approved_confirmed_create, drive_approved_confirmed_reconfigure,
    drive_approved_confirmed_start,
};
use self::drive::{drive_approved_create, drive_approved_reconfigure};

pub(super) struct DelegatedCommand {
    authority: DelegateRoomAuthority,
    request: DirectRoomAction,
    admission: PrincipalMutationAdmission,
    reply: oneshot::Sender<Result<serde_json::Value, PersistenceError>>,
}

impl RoomRuntime {
    /// Executes an action on the room actor, retaining moderation effects and floor progression.
    /// # Errors
    /// Rejects ended custody, budget exhaustion, conflicting retries and invalid targets.
    pub async fn execute_delegated_direct(
        &self,
        authority: &DelegateRoomAuthority,
        request: &DirectRoomAction,
    ) -> Result<serde_json::Value, PersistenceError> {
        let handle = self.handle(authority.room_id()).await;
        let (reply, response) = oneshot::channel();
        handle
            .mutations
            .try_send(RoomMutation::Delegated(DelegatedCommand {
                authority: authority.clone(),
                request: request.clone(),
                admission: self.principal_mutations.clone(),
                reply,
            }))
            .map_err(|error| PersistenceError::CommandRejected {
                code: if matches!(error, tokio::sync::mpsc::error::TrySendError::Full(_)) {
                    "room_busy"
                } else {
                    "room_unavailable"
                }
                .into(),
                message: "The room mutation queue is unavailable.".into(),
            })?;
        response
            .await
            .map_err(|_| PersistenceError::CommandUnresolved {
                code: "delegated_response_lost".into(),
                message: "The action response was lost; retry the exact request.".into(),
            })?
    }
}

pub(super) async fn execute(
    owners: RoomCommandOwners<'_>,
    command: DelegatedCommand,
) -> Option<crate::event_publication::PublicationAttempt> {
    execute_with_reply(
        owners,
        &command.admission,
        &command.authority,
        &command.request,
        |result| {
            let _ = command.reply.send(result);
        },
    )
    .await
}

pub(crate) async fn execute_with_reply(
    owners: RoomCommandOwners<'_>,
    admission: &PrincipalMutationAdmission,
    authority: &DelegateRoomAuthority,
    request: &DirectRoomAction,
    reply: impl FnOnce(Result<serde_json::Value, PersistenceError>),
) -> Option<crate::event_publication::PublicationAttempt> {
    if let DirectRoomAction::RequestDeleteTextChannel(delete) = request {
        let result = async {
            let _inflight = admission.acquire_inflight()?;
            let (result, debits) = owners
                .store
                .request_delegated_channel_delete(authority, delete, |owner, delegate, bytes| {
                    charge_request(admission, authority, request, owner, delegate, bytes)
                })
                .await?;
            if let Some(debits) = debits {
                for debit in debits {
                    debit.resolve();
                }
            }
            Ok(result)
        }
        .await;
        let publication = if result.is_ok() {
            Some(
                crate::event_publication::publish_durable_room_events(
                    owners.store,
                    owners.event_tx,
                    authority.room_id(),
                )
                .await,
            )
        } else {
            None
        };
        reply(result);
        return publication;
    }
    if let DirectRoomAction::RequestCreateAgent(params) = request {
        let result = async {
            let _inflight = admission.acquire_inflight()?;
            let owner = owners.store.delegated_create_owner(authority).await?;
            let selection = owners
                .provider_catalog
                .validate_creation(
                    authority.room_id(),
                    &owner.principal_id,
                    &params.request_id,
                    &create_selection_payload(params),
                )
                .await
                .map_err(|error| PersistenceError::CommandRejected {
                    code: error.code.into(),
                    message: error.message,
                })?;
            let draft: AgentSessionDraft = selection.into();
            let (mut result, debits) = owners
                .store
                .request_delegated_agent_create(
                    authority,
                    params,
                    &draft,
                    |owner, delegate, bytes| {
                        charge_request(admission, authority, request, owner, delegate, bytes)
                    },
                )
                .await?;
            if let Some(debits) = debits {
                for debit in debits {
                    debit.resolve();
                }
            }
            // An approved-but-undriven creation (owner approved while a drive was
            // lost) completes idempotently here; the receipt is the recorded state.
            if result.get("status").and_then(Value::as_str) == Some("in_progress")
                && result.get("action").and_then(Value::as_str) == Some("create_agent")
                && let Some(confirmation_id) = result
                    .get("confirmation_id")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            {
                let drive = drive_approved_create(
                    owners.store,
                    owners.provider_catalog,
                    authority.room_id(),
                    &confirmation_id,
                )
                .await?;
                result = drive.receipt;
            }
            Ok(result)
        }
        .await;
        let publication = if result.is_ok() {
            Some(
                crate::event_publication::publish_durable_room_events(
                    owners.store,
                    owners.event_tx,
                    authority.room_id(),
                )
                .await,
            )
        } else {
            None
        };
        reply(result);
        return publication;
    }
    if let DirectRoomAction::RequestReconfigureAgent(params) = request {
        let result = async {
            let _inflight = admission.acquire_inflight()?;
            let (owner, session) = owners
                .store
                .delegated_reconfigure_candidate(authority, &params.agent_id)
                .await?;
            let selection = owners
                .provider_catalog
                .validate_configuration(
                    authority.room_id(),
                    &owner.principal_id,
                    &params.request_id,
                    &session,
                    &reconfigure_selection_payload(params),
                )
                .await
                .map_err(|error| PersistenceError::CommandRejected {
                    code: error.code.into(),
                    message: error.message,
                })?;
            let draft: AgentSessionDraft = selection.into();
            let (mut result, debits) = owners
                .store
                .request_delegated_agent_reconfigure(
                    authority,
                    params,
                    &session.runtime_profile_key,
                    &draft,
                    |owner, delegate, bytes| {
                        charge_request(admission, authority, request, owner, delegate, bytes)
                    },
                )
                .await?;
            if let Some(debits) = debits {
                for debit in debits {
                    debit.resolve();
                }
            }
            // An approved-but-undriven reconfiguration (owner approved while a drive
            // was lost) completes idempotently here; the receipt is the recorded state.
            if result.get("status").and_then(Value::as_str) == Some("in_progress")
                && result.get("action").and_then(Value::as_str) == Some("reconfigure_agent")
                && let Some(confirmation_id) = result
                    .get("confirmation_id")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            {
                let drive = drive_approved_reconfigure(
                    owners.store,
                    owners.provider_catalog,
                    authority.room_id(),
                    &confirmation_id,
                )
                .await?;
                result = drive.receipt;
            }
            Ok(result)
        }
        .await;
        let publication = if result.is_ok() {
            Some(
                crate::event_publication::publish_durable_room_events(
                    owners.store,
                    owners.event_tx,
                    authority.room_id(),
                )
                .await,
            )
        } else {
            None
        };
        reply(result);
        return publication;
    }
    if let DirectRoomAction::RequestRemoveAgent(params) = request {
        let result = async {
            let _inflight = admission.acquire_inflight()?;
            let (result, debits) = owners
                .store
                .request_delegated_agent_removal(authority, params, |owner, delegate, bytes| {
                    charge_request(admission, authority, request, owner, delegate, bytes)
                })
                .await?;
            if let Some(debits) = debits {
                for debit in debits {
                    debit.resolve();
                }
            }
            Ok(result)
        }
        .await;
        let publication = if result.is_ok() {
            Some(
                crate::event_publication::publish_durable_room_events(
                    owners.store,
                    owners.event_tx,
                    authority.room_id(),
                )
                .await,
            )
        } else {
            None
        };
        reply(result);
        return publication;
    }
    if let DirectRoomAction::RequestKickParticipant(params) = request {
        let result = async {
            let _inflight = admission.acquire_inflight()?;
            let (result, debits) = owners
                .store
                .request_delegated_participant_kick(authority, params, |owner, delegate, bytes| {
                    charge_request(admission, authority, request, owner, delegate, bytes)
                })
                .await?;
            if let Some(debits) = debits {
                for debit in debits {
                    debit.resolve();
                }
            }
            Ok(result)
        }
        .await;
        let publication = if result.is_ok() {
            Some(
                crate::event_publication::publish_durable_room_events(
                    owners.store,
                    owners.event_tx,
                    authority.room_id(),
                )
                .await,
            )
        } else {
            None
        };
        reply(result);
        return publication;
    }
    if let DirectRoomAction::StartAgent(start) = request {
        let result = async {
            let _inflight = admission.acquire_inflight()?;
            let pending = owners
                .store
                .request_delegated_agent_start(authority, start, |owner, delegate, bytes| {
                    charge_request(admission, authority, request, owner, delegate, bytes)
                })
                .await?;
            match pending {
                Some((result, debits)) => {
                    if let Some(debits) = debits {
                        for debit in debits {
                            debit.resolve();
                        }
                    }
                    Ok(Some(result))
                }
                None => Ok(None),
            }
        }
        .await;
        match result {
            Ok(Some(result)) => {
                let publication = Some(
                    crate::event_publication::publish_durable_room_events(
                        owners.store,
                        owners.event_tx,
                        authority.room_id(),
                    )
                    .await,
                );
                reply(Ok(result));
                return publication;
            }
            Ok(None) => {}
            Err(error) => {
                reply(Err(error));
                return None;
            }
        }
    }
    if let DirectRoomAction::RequestCreateInvite(params) = request {
        return crate::delegated_invite_runtime::request(
            owners, admission, authority, request, params, reply,
        )
        .await;
    }
    let (reserved, mut progress) = tokio::sync::mpsc::unbounded_channel();
    let execution = apply(
        owners.store,
        owners.provider_adapter,
        admission,
        authority,
        request,
        owners.lifecycle_commands,
        Some(&reserved),
    );
    tokio::pin!(execution);
    let mut reply = Some(reply);
    let execution = tokio::select! {
        result = &mut execution => result,
        Some(operation_id) = progress.recv() => {
            if let Some(reply) = reply.take() {
                reply(Ok(in_progress(request, &operation_id)));
            }
            execution.await
        }
    };
    let publication = publish(owners, &execution.execution).await;
    if let Some(reply) = reply {
        reply(execution.into_result());
    }
    publication
}
pub(crate) fn charge_request(
    admission: &PrincipalMutationAdmission,
    authority: &DelegateRoomAuthority,
    request: &DirectRoomAction,
    owner: &str,
    delegate: &str,
    bytes: usize,
) -> Result<[crate::principal_mutation_admission::MutationDebit; 2], PersistenceError> {
    let hash = canonical_payload_hash(&json!({"delegate":delegate,"request":request}));
    let identity = MutationIdentity::new(
        authority.room_id(),
        request.request_id(),
        request.name(),
        &hash,
    );
    Ok([
        admission.charge(owner, identity, bytes)?,
        admission.charge(delegate, identity, bytes)?,
    ])
}
