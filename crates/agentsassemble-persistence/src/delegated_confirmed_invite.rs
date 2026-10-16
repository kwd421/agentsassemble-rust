//! Owner-confirmed HUMAN room-invite creation binds the owner's own invite policy.
//!
//! The AI supplies only the invite fields the owner's own human-invite creation accepts
//! from a person; room, creator identity and credentials come from authentication and
//! server state. The request path stores the validated policy without mutation and
//! refuses an owner whose provenance cannot become the owner's own room-manager
//! authority. Approval revalidates both authorities, re-runs the same policy validation
//! and inserts the invite through the same core the owner's own creation uses. The
//! minted token, join code and link are never stored here: the server crate mints them
//! and returns them only in the owner's live acknowledgement, so the stored request,
//! receipt and events stay secret-free.
use chrono::{DateTime, TimeDelta, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::delegated_confirmation::{Binding, create_in, rejected};
use crate::delegated_confirmation_lifecycle::{
    finish_in, item, load_in, receipt, retry_in, validate_in,
};
use crate::{
    DelegateRoomAuthority, HumanInvite, LocalRoomManagerAuthority, NewHumanInvite,
    PersistenceError, RoomManagerAuthority, RoomUserIdentity, SqliteStore,
};
use agentsassemble_domain::{
    ConfirmationDecision, ConfirmationState, ConfirmedHumanInvite, ConfirmedRoomAction,
    DecideDelegatedAction, DelegatedActionRequest, InviteScope, RequestCreateInvite, RoomEvent,
    canonical_payload_hash,
};
use sqlx::{Row, Sqlite, Transaction};

const MAX_INVITE_LABEL_CHARS: usize = 128;

/// Normalised owner-accepted invite policy bound by one delegated request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DelegatedInvitePolicy {
    pub display_name: String,
    pub invite_scope: InviteScope,
    pub ttl_seconds: i64,
    pub max_uses: i64,
}

/// The already-minted credential identity and window for one approved invite. The server
/// crate owns raw credential issuance; persistence receives only fingerprints and the
/// public policy it must re-validate.
pub struct MintedHumanInvite {
    pub signed_token_fingerprint: [u8; 32],
    pub join_code_fingerprint: [u8; 32],
    pub base_participant_id: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

/// What one approved-invite drive produced. `invite` is present only when this call
/// minted and stored the row, so a later replay returns the receipt without a link.
pub struct ApprovedInviteDrive {
    pub receipt: Value,
    pub state: ConfirmationState,
    pub new_events: Vec<RoomEvent>,
    pub invite: Option<HumanInvite>,
}

/// Validates and normalises the AI-supplied invite fields as the owner's own human
/// invite creation does: an empty label becomes `Guest`, the label is one single-line
/// value within its limit, the scope is an exact supported value, the lifetime is
/// positive and representable as a checked duration exactly as the owner route
/// requires, and the use limit is non-negative.
/// # Errors
/// Rejects `invalid_human_invite` for any out-of-policy field, and `bad_request` for
/// a lifetime the owner route itself refuses as over-limit.
pub fn delegated_invite_policy(
    params: &RequestCreateInvite,
) -> Result<DelegatedInvitePolicy, PersistenceError> {
    let label = params.display_name.as_deref().unwrap_or_default().trim();
    let display_name = if label.is_empty() { "Guest" } else { label };
    if display_name.chars().count() > MAX_INVITE_LABEL_CHARS || display_name.contains(['\r', '\n'])
    {
        return Err(rejected(
            "invalid_human_invite",
            "Human invite policy is invalid.",
        ));
    }
    let invite_scope = match params.invite_scope.as_str() {
        "read_write" => InviteScope::ReadWrite,
        "read_only" => InviteScope::ReadOnly,
        _ => {
            return Err(rejected(
                "invalid_human_invite",
                "Human invite policy is invalid.",
            ));
        }
    };
    if params.ttl_seconds <= 0 || params.max_uses < 0 {
        return Err(rejected(
            "invalid_human_invite",
            "Human invite policy is invalid.",
        ));
    }
    if TimeDelta::try_seconds(params.ttl_seconds).is_none() {
        return Err(rejected("bad_request", "ttl_seconds must be positive."));
    }
    Ok(DelegatedInvitePolicy {
        display_name: display_name.to_owned(),
        invite_scope,
        ttl_seconds: params.ttl_seconds,
        max_uses: params.max_uses,
    })
}

fn consequences(policy: &DelegatedInvitePolicy) -> String {
    let scope = match policy.invite_scope {
        InviteScope::ReadWrite => "read and write messages in",
        InviteScope::ReadOnly => "read messages in",
    };
    format!(
        "Creates a room invitation that lets whoever holds the link or join code {scope} this room. You see the link once in this acknowledgement; it is never posted to the room and cannot be retrieved later."
    )
}

/// The deciding session recorded with one approved HUMAN invite. Only a session
/// that could create a human invite through the owner's own route right now may
/// approve: the desktop's own local session, or an exact central-owner operator
/// session. An ordinary paired operator browser is never recorded here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum InviteApprover {
    /// The desktop's own local session, pinned to the current local-bootstrap
    /// lineage. Recorded from local socket provenance, never from a participant id.
    Local {
        server_id: String,
        authority_lineage_id: String,
        room_uid: Uuid,
        user_id: String,
        participant_id: String,
    },
    /// An exact central-owner operator session, pinned by session, device, origin
    /// and the manager identity it resolved to.
    Operator {
        fingerprint: [u8; 32],
        device: [u8; 32],
        origin: String,
        user_id: String,
        participant_id: String,
    },
}

/// One invite approval decision with the session that made it. The mint
/// transaction re-resolves exactly this session and mints under its authority, so
/// the link can only be minted for, and delivered to, the session that approved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct RecordedInviteDecision {
    pub(crate) decision: DecideDelegatedAction,
    pub(crate) approver: InviteApprover,
}

/// Whether a stored invite decision encodes the same owner decision. Recorded
/// approvals compare their decision and ignore the bound approver, so an exact
/// decision replays its receipt; anything else conflicts.
pub(crate) fn recorded_invite_decision_matches(
    previous: &str,
    decision: &DecideDelegatedAction,
) -> bool {
    if let Ok(recorded) = serde_json::from_str::<RecordedInviteDecision>(previous) {
        return recorded.decision == *decision;
    }
    serde_json::from_str::<DecideDelegatedAction>(previous).is_ok_and(|stored| stored == *decision)
}

/// Re-resolves the recorded approver into the owner's own room-manager authority
/// inside the mint transaction: current local-bootstrap lineage for the desktop's
/// own local session, or the exact live central-owner operator session. Anything
/// else refuses, so the mint can only run under the session that approved.
/// # Errors
/// Refuses `owner_authority_unavailable` for an unresolvable or ineligible
/// approver; propagates storage failures.
pub(crate) async fn approver_manager_authority(
    tx: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    approver: &InviteApprover,
) -> Result<RoomManagerAuthority, PersistenceError> {
    match approver {
        InviteApprover::Local {
            server_id,
            authority_lineage_id,
            room_uid,
            user_id,
            participant_id,
        } => {
            let expected = LocalRoomManagerAuthority {
                server_id: server_id.clone(),
                authority_lineage_id: authority_lineage_id.clone(),
                room_uid: *room_uid,
                manager: RoomUserIdentity {
                    room_id: room_id.to_owned(),
                    user_id: user_id.clone(),
                    participant_id: participant_id.clone(),
                },
            };
            expected.resolve(tx).await.map_err(|error| match error {
                PersistenceError::CommandRejected { .. } => unavailable(),
                other => other,
            })?;
            Ok(RoomManagerAuthority::Local(expected))
        }
        InviteApprover::Operator {
            fingerprint,
            device,
            origin,
            user_id,
            participant_id,
        } => {
            let session = crate::operator_pairing::resolve_operator_session(
                tx,
                fingerprint,
                device,
                origin,
                Utc::now(),
            )
            .await
            .map_err(|error| match error {
                PersistenceError::CommandRejected { .. } => unavailable(),
                other => other,
            })?;
            if !session.is_central_owner()
                || session.principal().room_id != room_id
                || session.principal().principal_id != *user_id
                || session.principal().participant_id != *participant_id
            {
                return Err(unavailable());
            }
            Ok(RoomManagerAuthority::Operator(Box::new(session)))
        }
    }
}

fn unavailable() -> PersistenceError {
    rejected(
        "owner_authority_unavailable",
        "The bound owner's authority is unavailable.",
    )
}

impl SqliteStore {
    /// Stores only an immutable human-invite confirmation, binding the normalised owner
    /// policy. The owner's provenance must already resolve to the owner's own
    /// room-manager authority, otherwise the request is refused rather than queued. Exact
    /// retries never debit again.
    /// # Errors
    /// Rejects stale custody, an unprovable owner authority, conflicting retries, invalid
    /// policy and exhausted budgets.
    pub async fn request_delegated_human_invite<A>(
        &self,
        authority: &DelegateRoomAuthority,
        params: &RequestCreateInvite,
        admit: impl FnOnce(&str, &str, usize) -> Result<A, PersistenceError>,
    ) -> Result<(Value, Option<A>), PersistenceError> {
        if !Uuid::parse_str(&params.request_id).is_ok_and(|id| !id.is_nil()) {
            return Err(rejected(
                "bad_request",
                "A nonzero request UUID is required.",
            ));
        }
        let policy = delegated_invite_policy(params)?;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let action = ConfirmedRoomAction::CreateHumanInvite(ConfirmedHumanInvite {
            request_id: params.request_id.clone(),
            params: params.clone(),
        });
        if let Some(result) = retry_in(&mut tx, authority, &params.request_id, &action).await? {
            tx.commit().await?;
            return Ok((result, None));
        }
        let proof = crate::delegated_pin::DelegatedRoomActionAuthority::direct(
            &mut tx,
            authority,
            "request_create_invite",
        )
        .await?;
        crate::delegation_owner::manager_authority_for_delegate(
            &mut tx,
            authority.room_id(),
            authority.delegate_id(),
        )
        .await?;
        crate::command_admission::existing_request_identity(
            &mut tx,
            authority.room_id(),
            &proof.delegate.principal_id,
            &params.request_id,
            "request_create_invite",
            &canonical_payload_hash(&serde_json::to_value(&action)?),
        )
        .await
        .map_err(|error| match error {
            PersistenceError::CommandConflict => rejected(
                "request_conflict",
                "This request ID already identifies another action.",
            ),
            other => other,
        })?;
        let result = create_in(
            &mut tx,
            authority,
            &proof,
            action,
            policy.display_name.clone(),
            &consequences(&policy),
            admit,
        )
        .await?;
        tx.commit().await?;
        Ok(result)
    }

    /// Loads the bound invite request, its owner reference and current receipt.
    /// # Errors
    /// Rejects missing confirmations, other actions and storage failures.
    pub async fn approved_invite_snapshot(
        &self,
        room_id: &str,
        confirmation_id: &str,
    ) -> Result<(DelegatedActionRequest, Value, Value), PersistenceError> {
        let mut tx = self.pool.begin().await?;
        let row = load_in(&mut tx, room_id, confirmation_id).await?;
        let request = item(&row)?;
        if !matches!(request.action, ConfirmedRoomAction::CreateHumanInvite(_)) {
            return Err(rejected(
                "bad_request",
                "A human invite creation is required.",
            ));
        }
        let binding: Binding = serde_json::from_str(row.get("binding_json"))?;
        let receipt = receipt(&row)?;
        tx.commit().await?;
        Ok((
            request,
            serde_json::to_value(binding.owner_reference)?,
            receipt,
        ))
    }

    /// Records a drive-time failure as the terminal receipt.
    /// # Errors
    /// Propagates storage failures; a non-executing request returns its receipt.
    pub async fn fail_approved_human_invite(
        &self,
        room_id: &str,
        confirmation_id: &str,
        code: &str,
        message: &str,
    ) -> Result<ApprovedInviteDrive, PersistenceError> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let row = load_in(&mut tx, room_id, confirmation_id).await?;
        let request = item(&row)?;
        if request.state != ConfirmationState::Executing {
            let receipt = receipt(&row)?;
            tx.commit().await?;
            return Ok(ApprovedInviteDrive {
                receipt,
                state: request.state,
                new_events: Vec::new(),
                invite: None,
            });
        }
        let receipt = json!({"status":"rejected","code":code,"message":message,"retryable":false});
        finish_in(&mut tx, room_id, &row, ConfirmationState::Failed, &receipt).await?;
        tx.commit().await?;
        self.notify_room_directory_changed();
        Ok(ApprovedInviteDrive {
            receipt,
            state: ConfirmationState::Failed,
            new_events: Vec::new(),
            invite: None,
        })
    }

    /// Records an approval whose bound session ended before the mint as cancelled:
    /// the approving authority is gone, so nothing may mint and the owner starts
    /// over with a fresh request.
    /// # Errors
    /// Propagates storage failures; a non-executing request returns its receipt.
    pub async fn cancel_approved_human_invite(
        &self,
        room_id: &str,
        confirmation_id: &str,
    ) -> Result<ApprovedInviteDrive, PersistenceError> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let row = load_in(&mut tx, room_id, confirmation_id).await?;
        let request = item(&row)?;
        if request.state != ConfirmationState::Executing {
            let receipt = receipt(&row)?;
            tx.commit().await?;
            return Ok(ApprovedInviteDrive {
                receipt,
                state: request.state,
                new_events: Vec::new(),
                invite: None,
            });
        }
        let receipt = json!({"status":"rejected","code":"confirmation_cancelled","message":"The owner or delegate authority ended before execution.","retryable":false});
        finish_in(
            &mut tx,
            room_id,
            &row,
            ConfirmationState::Cancelled,
            &receipt,
        )
        .await?;
        tx.commit().await?;
        self.notify_room_directory_changed();
        Ok(ApprovedInviteDrive {
            receipt,
            state: ConfirmationState::Cancelled,
            new_events: Vec::new(),
            invite: None,
        })
    }

    /// Executes one approved HUMAN invite creation through the owner's own invite core.
    /// Idempotent: a request that already left `executing` returns its receipt without
    /// minting or storing a credential. The credential identity is supplied by the
    /// server crate; nothing secret is stored on the request or receipt.
    /// # Errors
    /// Returns the recorded terminal receipt for rejections; storage failures roll back
    /// without changing state so a later retry can re-drive.
    pub async fn drive_approved_human_invite(
        &self,
        room_id: &str,
        confirmation_id: &str,
        minted: &MintedHumanInvite,
    ) -> Result<ApprovedInviteDrive, PersistenceError> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let row = load_in(&mut tx, room_id, confirmation_id).await?;
        let request = item(&row)?;
        let stored = match &request.action {
            ConfirmedRoomAction::CreateHumanInvite(invite) => invite,
            _ => {
                return Err(rejected(
                    "bad_request",
                    "A human invite creation is required.",
                ));
            }
        };
        if request.state != ConfirmationState::Executing {
            if request.state == ConfirmationState::Pending {
                return Err(rejected(
                    "permission_denied",
                    "The owner has not approved this execution.",
                ));
            }
            let receipt = receipt(&row)?;
            tx.commit().await?;
            return Ok(ApprovedInviteDrive {
                receipt,
                state: request.state,
                new_events: Vec::new(),
                invite: None,
            });
        }
        let recorded = match row.get::<Option<&str>, _>("decision_json") {
            Some(encoded) => match serde_json::from_str::<RecordedInviteDecision>(encoded) {
                Ok(recorded) => recorded,
                Err(_) => {
                    // No bound approver (a corrupt or pre-binding row): fail closed.
                    tx.commit().await?;
                    return self
                        .cancel_approved_human_invite(room_id, confirmation_id)
                        .await;
                }
            },
            None => {
                return Err(rejected("permission_denied", "The owner has not approved."));
            }
        };
        if recorded.decision.decision != ConfirmationDecision::Approve {
            return Err(rejected(
                "permission_denied",
                "The owner has not approved this execution.",
            ));
        }
        let proof = match validate_in(&mut tx, room_id, &row).await {
            Ok(proof) => proof,
            Err(
                PersistenceError::CommandRejected { .. }
                | PersistenceError::ParticipantMissing
                | PersistenceError::RoomMissing,
            ) => {
                let receipt = json!({"status":"rejected","code":"confirmation_cancelled","message":"The owner or delegate authority ended before execution.","retryable":false});
                finish_in(
                    &mut tx,
                    room_id,
                    &row,
                    ConfirmationState::Cancelled,
                    &receipt,
                )
                .await?;
                tx.commit().await?;
                self.notify_room_directory_changed();
                return Ok(ApprovedInviteDrive {
                    receipt,
                    state: ConfirmationState::Cancelled,
                    new_events: Vec::new(),
                    invite: None,
                });
            }
            Err(error) => return Err(error),
        };
        // Re-run the same policy validation and owner provenance mapping at approval.
        let policy = match delegated_invite_policy(&stored.params) {
            Ok(policy) => policy,
            Err(PersistenceError::CommandRejected { code, message }) => {
                let receipt =
                    json!({"status":"rejected","code":code,"message":message,"retryable":false});
                finish_in(&mut tx, room_id, &row, ConfirmationState::Failed, &receipt).await?;
                tx.commit().await?;
                self.notify_room_directory_changed();
                return Ok(ApprovedInviteDrive {
                    receipt,
                    state: ConfirmationState::Failed,
                    new_events: Vec::new(),
                    invite: None,
                });
            }
            Err(error) => return Err(error),
        };
        // The mint runs under the recorded approving session's own authority,
        // re-resolved here: a session revoked after approval mints nothing.
        let manager = match approver_manager_authority(&mut tx, room_id, &recorded.approver).await {
            Ok(manager) => manager,
            Err(PersistenceError::CommandRejected { .. }) => {
                tx.commit().await?;
                return self
                    .cancel_approved_human_invite(room_id, confirmation_id)
                    .await;
            }
            Err(error) => return Err(error),
        };
        let first_seq = crate::room_event_sequence::next_sequence(&mut tx, room_id).await?;
        let invite = crate::human_invites::create_human_invite_in(
            &mut tx,
            &manager,
            NewHumanInvite {
                signed_token_fingerprint: minted.signed_token_fingerprint,
                join_code_fingerprint: minted.join_code_fingerprint,
                base_participant_id: minted.base_participant_id.clone(),
                display_name: policy.display_name.clone(),
                invite_scope: policy.invite_scope,
                max_uses: policy.max_uses,
                expires_at: minted.expires_at,
                created_at: minted.created_at,
            },
        )
        .await?;
        let mut events: Vec<RoomEvent> = Vec::new();
        let mut system = crate::delegated_pin::attributed_action_event(
            &mut tx,
            proof.attribution(),
            &stored.request_id,
            "create_human_invite",
            "",
            "created a room invite",
        )
        .await?;
        system
            .extra
            .insert("confirmation_id".into(), json!(request.confirmation_id));
        crate::room_turns::support::insert_event(&mut tx, &system).await?;
        events.push(system.clone());
        crate::delegated_event_attribution::attribute_since(
            &mut tx,
            &proof.delegate,
            &proof.owner.participant_id,
            Some(&request.confirmation_id),
            first_seq,
            &mut events,
        )
        .await?;
        let receipt = json!({"status":"succeeded","request_id":stored.request_id,"action":"create_human_invite","confirmation_id":request.confirmation_id,"event_id":system.id});
        finish_in(
            &mut tx,
            room_id,
            &row,
            ConfirmationState::Succeeded,
            &receipt,
        )
        .await?;
        tx.commit().await?;
        self.notify_room_directory_changed();
        Ok(ApprovedInviteDrive {
            receipt,
            state: ConfirmationState::Succeeded,
            new_events: events,
            invite: Some(invite),
        })
    }
}
