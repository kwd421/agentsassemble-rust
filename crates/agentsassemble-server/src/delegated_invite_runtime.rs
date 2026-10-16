//! Server-owned minting for one approved delegated HUMAN room invite.
//!
//! The room actor's request branch stores the owner-bound confirmation; the owner's own
//! socket decision commits it. This module mints the credential in the server crate and
//! drives the persistence core in one transaction, then returns the link only for the
//! owner's live acknowledgement. Raw credentials exist only in memory here and are never
//! persisted to the request, receipt, events or any AI-visible result.
use agentsassemble_domain::{
    ConfirmationState, ConfirmedRoomAction, DirectRoomAction, RequestCreateInvite,
};
use agentsassemble_persistence::{
    CommandOutcome, DelegateRoomAuthority, MintedHumanInvite, PersistenceError,
    delegated_invite_policy,
};
use serde_json::{Value, json};

use crate::AppState;
use crate::human_invite_credentials::{HumanInviteCredentialDraft, format_invite_timestamp};
use crate::principal_mutation_admission::PrincipalMutationAdmission;
use crate::room_runtime::RoomCommandOwners;

/// Opens one delegated human-invite confirmation on the room actor, charging both
/// principals' existing write/admission budgets exactly as the other confirmed actions
/// do.
pub(crate) async fn request(
    owners: RoomCommandOwners<'_>,
    admission: &PrincipalMutationAdmission,
    authority: &DelegateRoomAuthority,
    request: &DirectRoomAction,
    params: &RequestCreateInvite,
    reply: impl FnOnce(Result<Value, PersistenceError>),
) -> Option<crate::event_publication::PublicationAttempt> {
    let result = async {
        let _inflight = admission.acquire_inflight()?;
        let (result, debits) = owners
            .store
            .request_delegated_human_invite(authority, params, |owner, delegate, bytes| {
                crate::room_runtime::delegated_direct::charge_request(
                    admission, authority, request, owner, delegate, bytes,
                )
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
    publication
}

/// Mints and stores an approved human invite after the decision committed, adding the
/// one-time credential to the owner's live acknowledgement only. A deduplicated decision
/// or a request that already left `executing` returns no credential: the owner who lost
/// the acknowledgement creates another invite themself.
pub(crate) async fn drive_approved_confirmed_invite(
    state: &AppState,
    room_id: &str,
    outcome: &mut CommandOutcome,
) {
    if outcome.deduplicated {
        return;
    }
    if outcome.result.get("state").and_then(Value::as_str) != Some("executing")
        || outcome
            .result
            .get("receipt")
            .and_then(|receipt| receipt.get("action"))
            .and_then(Value::as_str)
            != Some("create_human_invite")
    {
        return;
    }
    let Some(confirmation_id) = outcome
        .result
        .get("confirmation_id")
        .and_then(Value::as_str)
        .map(str::to_owned)
    else {
        return;
    };
    match mint_and_drive(state, room_id, &confirmation_id).await {
        Ok(Some(invite)) => {
            outcome.result["invite"] = invite;
        }
        Ok(None) => {}
        Err(error) => {
            tracing::warn!(error = ?error, room_id, confirmation_id, "approved delegated invite did not complete");
        }
    }
}

async fn mint_and_drive(
    state: &AppState,
    room_id: &str,
    confirmation_id: &str,
) -> Result<Option<Value>, PersistenceError> {
    let (request, _owner_reference, _receipt) = state
        .store
        .approved_invite_snapshot(room_id, confirmation_id)
        .await?;
    if request.state != ConfirmationState::Executing {
        return Ok(None);
    }
    let ConfirmedRoomAction::CreateHumanInvite(invite) = &request.action else {
        return Err(rejected(
            "bad_request",
            "A human invite creation is required.",
        ));
    };
    let policy = match delegated_invite_policy(&invite.params) {
        Ok(policy) => policy,
        Err(PersistenceError::CommandRejected { code, message }) => {
            state
                .store
                .fail_approved_human_invite(room_id, confirmation_id, &code, &message)
                .await?;
            return Ok(None);
        }
        Err(error) => return Err(error),
    };
    let Some(ingress) = state.public_ingress.ready_snapshot() else {
        state
            .store
            .fail_approved_human_invite(
                room_id,
                confirmation_id,
                "public_ingress_not_ready",
                "Public ingress must be ready before creating an invite.",
            )
            .await?;
        return Ok(None);
    };
    let epoch = state.store.registration_epoch().await?;
    let Some(created_at) =
        chrono::DateTime::from_timestamp_micros(chrono::Utc::now().timestamp_micros())
    else {
        return Err(rejected(
            "invite_window_invalid",
            "The invite window is invalid.",
        ));
    };
    let Some(ttl) = chrono::TimeDelta::try_seconds(policy.ttl_seconds) else {
        state
            .store
            .fail_approved_human_invite(
                room_id,
                confirmation_id,
                "invite_window_invalid",
                "The invite expiry is out of range.",
            )
            .await?;
        return Ok(None);
    };
    let Some(expires_at) = created_at.checked_add_signed(ttl) else {
        state
            .store
            .fail_approved_human_invite(
                room_id,
                confirmation_id,
                "invite_window_invalid",
                "The invite expiry is out of range.",
            )
            .await?;
        return Ok(None);
    };
    let base_participant_id = format!("guest-{}", uuid::Uuid::new_v4().simple());
    let credentials = match state
        .human_invite_credentials
        .issue(&HumanInviteCredentialDraft {
            room_url: ingress.local_url.clone(),
            public_room_url: ingress.public_url.clone(),
            room_id: room_id.to_owned(),
            base_participant_id: base_participant_id.clone(),
            display_name: policy.display_name.clone(),
            invite_scope: policy.invite_scope,
            issued_at: created_at,
            expires_at,
        }) {
        Ok(credentials) => credentials,
        Err(_) => {
            state
                .store
                .fail_approved_human_invite(
                    room_id,
                    confirmation_id,
                    "invite_credentials_unavailable",
                    "Invite credentials could not be issued.",
                )
                .await?;
            return Ok(None);
        }
    };
    // Mirrors the owner's own member-join URL construction in `human_invite_manager_web`.
    let join_url = match epoch.as_deref() {
        Some(epoch) => state
            .central_directory
            .member_entry_url(
                state.central_host_identity.server_id(),
                epoch,
                credentials.join_code(),
            )
            .map_err(|_| {
                rejected(
                    "invite_join_url_unavailable",
                    "Invite join URL is unavailable.",
                )
            })?,
        None => format!(
            "{}/join?token={}",
            ingress.public_url,
            credentials.join_code()
        ),
    };
    let drive = state
        .store
        .drive_approved_human_invite(
            room_id,
            confirmation_id,
            &MintedHumanInvite {
                signed_token_fingerprint: *credentials.signed_token_fingerprint(),
                join_code_fingerprint: *credentials.join_code_fingerprint(),
                base_participant_id,
                created_at,
                expires_at,
            },
        )
        .await;
    let drive = match drive {
        Ok(drive) => drive,
        Err(PersistenceError::CommandRejected { code, message }) => {
            state
                .store
                .fail_approved_human_invite(room_id, confirmation_id, &code, &message)
                .await?;
            return Ok(None);
        }
        Err(error) => return Err(error),
    };
    let Some(stored) = drive.invite else {
        return Ok(None);
    };
    state.rooms.notify_room_publication(room_id).await;
    Ok(Some(json!({
        "invite_id": stored.invite_id,
        "invite_token": credentials.invite_token(),
        "join_code": credentials.join_code(),
        "join_url": join_url,
        "display_name": policy.display_name,
        "invite_scope": invite_scope_wire(policy.invite_scope),
        "max_uses": policy.max_uses,
        "expires_at": format_invite_timestamp(expires_at),
    })))
}

const fn invite_scope_wire(scope: agentsassemble_domain::InviteScope) -> &'static str {
    match scope {
        agentsassemble_domain::InviteScope::ReadWrite => "read_write",
        agentsassemble_domain::InviteScope::ReadOnly => "read_only",
    }
}

fn rejected(code: &'static str, message: &'static str) -> PersistenceError {
    PersistenceError::CommandRejected {
        code: code.into(),
        message: message.into(),
    }
}
