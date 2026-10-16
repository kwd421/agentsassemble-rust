//! Owner-confirmed delegated request branches: each stores the owner-bound
//! confirmation on the room actor under the shared admission plumbing.
use std::future::Future;

use agentsassemble_domain::RoomEvent;
use agentsassemble_domain::{
    AgentSessionDraft, DeleteTextChannel, DirectRoomAction, RequestCreateAgent,
    RequestKickParticipant, RequestReconfigureAgent, RequestRemoveAgent,
};
use agentsassemble_persistence::{
    DelegateRoomAuthority, PersistenceError, SqliteStore, create_selection_payload,
    reconfigure_selection_payload,
};
use serde_json::Value;

use crate::principal_mutation_admission::{MutationDebit, PrincipalMutationAdmission};
use crate::room_runtime::RoomCommandOwners;

use super::charge_request;
use super::drive::{drive_approved_create, drive_approved_reconfigure};

type Debits = [MutationDebit; 2];
type RequestOutput = Result<(serde_json::Value, Option<Debits>), PersistenceError>;

/// The shared request-branch plumbing: current in-flight admission, resolution
/// of the returned owner+delegate budget debits, durable publication only on
/// success and the exact reply with the branch result.
pub(crate) async fn confirmed_request(
    store: &SqliteStore,
    event_tx: &tokio::sync::broadcast::Sender<RoomEvent>,
    room_id: &str,
    admission: &PrincipalMutationAdmission,
    reply: impl FnOnce(Result<serde_json::Value, PersistenceError>),
    branch: impl Future<Output = RequestOutput>,
) -> Option<crate::event_publication::PublicationAttempt> {
    let result = async {
        let _inflight = admission.acquire_inflight()?;
        let (result, debits) = branch.await?;
        if let Some(debits) = debits {
            for debit in debits {
                debit.resolve();
            }
        }
        Ok(result)
    }
    .await;
    let publication = if result.is_ok() {
        Some(crate::event_publication::publish_durable_room_events(store, event_tx, room_id).await)
    } else {
        None
    };
    reply(result);
    publication
}

pub(crate) async fn create_agent(
    owners: RoomCommandOwners<'_>,
    admission: &PrincipalMutationAdmission,
    authority: &DelegateRoomAuthority,
    request: &DirectRoomAction,
    params: &RequestCreateAgent,
    reply: impl FnOnce(Result<serde_json::Value, PersistenceError>),
) -> Option<crate::event_publication::PublicationAttempt> {
    let store = owners.store;
    let catalog = owners.provider_catalog;
    confirmed_request(
        store,
        owners.event_tx,
        authority.room_id(),
        admission,
        reply,
        async {
            let owner = store.delegated_create_owner(authority).await?;
            let selection = catalog
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
            let (mut result, debits) = store
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
                result =
                    drive_approved_create(store, catalog, authority.room_id(), &confirmation_id)
                        .await?
                        .receipt;
            }
            Ok((result, None))
        },
    )
    .await
}

pub(crate) async fn reconfigure_agent(
    owners: RoomCommandOwners<'_>,
    admission: &PrincipalMutationAdmission,
    authority: &DelegateRoomAuthority,
    request: &DirectRoomAction,
    params: &RequestReconfigureAgent,
    reply: impl FnOnce(Result<serde_json::Value, PersistenceError>),
) -> Option<crate::event_publication::PublicationAttempt> {
    let store = owners.store;
    let catalog = owners.provider_catalog;
    confirmed_request(
        store,
        owners.event_tx,
        authority.room_id(),
        admission,
        reply,
        async {
            let (owner, session) = store
                .delegated_reconfigure_candidate(authority, &params.agent_id)
                .await?;
            let selection = catalog
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
            let (mut result, debits) = store
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
                result = drive_approved_reconfigure(
                    store,
                    catalog,
                    authority.room_id(),
                    &confirmation_id,
                )
                .await?
                .receipt;
            }
            Ok((result, None))
        },
    )
    .await
}

pub(crate) async fn remove_agent(
    owners: RoomCommandOwners<'_>,
    admission: &PrincipalMutationAdmission,
    authority: &DelegateRoomAuthority,
    request: &DirectRoomAction,
    params: &RequestRemoveAgent,
    reply: impl FnOnce(Result<serde_json::Value, PersistenceError>),
) -> Option<crate::event_publication::PublicationAttempt> {
    let store = owners.store;
    confirmed_request(
        store,
        owners.event_tx,
        authority.room_id(),
        admission,
        reply,
        async {
            store
                .request_delegated_agent_removal(authority, params, |owner, delegate, bytes| {
                    charge_request(admission, authority, request, owner, delegate, bytes)
                })
                .await
        },
    )
    .await
}

pub(crate) async fn kick_participant(
    owners: RoomCommandOwners<'_>,
    admission: &PrincipalMutationAdmission,
    authority: &DelegateRoomAuthority,
    request: &DirectRoomAction,
    params: &RequestKickParticipant,
    reply: impl FnOnce(Result<serde_json::Value, PersistenceError>),
) -> Option<crate::event_publication::PublicationAttempt> {
    let store = owners.store;
    confirmed_request(
        store,
        owners.event_tx,
        authority.room_id(),
        admission,
        reply,
        async {
            store
                .request_delegated_participant_kick(authority, params, |owner, delegate, bytes| {
                    charge_request(admission, authority, request, owner, delegate, bytes)
                })
                .await
        },
    )
    .await
}

pub(crate) async fn delete_text_channel(
    owners: RoomCommandOwners<'_>,
    admission: &PrincipalMutationAdmission,
    authority: &DelegateRoomAuthority,
    request: &DirectRoomAction,
    delete: &DeleteTextChannel,
    reply: impl FnOnce(Result<serde_json::Value, PersistenceError>),
) -> Option<crate::event_publication::PublicationAttempt> {
    let store = owners.store;
    confirmed_request(
        store,
        owners.event_tx,
        authority.room_id(),
        admission,
        reply,
        async {
            store
                .request_delegated_channel_delete(authority, delete, |owner, delegate, bytes| {
                    charge_request(admission, authority, request, owner, delegate, bytes)
                })
                .await
        },
    )
    .await
}
