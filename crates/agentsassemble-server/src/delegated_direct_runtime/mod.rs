use agentsassemble_domain::{DirectRoomAction, canonical_payload_hash};
use agentsassemble_persistence::{DelegateRoomAuthority, PersistenceError};
use serde_json::json;
use tokio::sync::oneshot;

use super::{RoomCommandOwners, RoomMutation, RoomRuntime};
use crate::principal_mutation_admission::{MutationIdentity, PrincipalMutationAdmission};

mod apply;
mod drive;
mod requests;

pub(crate) use self::apply::apply;
use self::apply::{in_progress, publish};
pub(crate) use self::drive::{
    drive_approved_confirmed_create, drive_approved_confirmed_reconfigure,
    drive_approved_confirmed_start,
};
pub(crate) use self::requests::confirmed_request;

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
        return requests::delete_text_channel(owners, admission, authority, request, delete, reply)
            .await;
    }
    if let DirectRoomAction::RequestCreateAgent(params) = request {
        return requests::create_agent(owners, admission, authority, request, params, reply).await;
    }
    if let DirectRoomAction::RequestReconfigureAgent(params) = request {
        return requests::reconfigure_agent(owners, admission, authority, request, params, reply)
            .await;
    }
    if let DirectRoomAction::RequestRemoveAgent(params) = request {
        return requests::remove_agent(owners, admission, authority, request, params, reply).await;
    }
    if let DirectRoomAction::RequestKickParticipant(params) = request {
        return requests::kick_participant(owners, admission, authority, request, params, reply)
            .await;
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
