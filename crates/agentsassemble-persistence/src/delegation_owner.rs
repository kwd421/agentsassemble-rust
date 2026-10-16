//! Server-only provenance. Identity alone never selects a session or grants authority.
use agentsassemble_domain::{AuthenticatedPrincipal, ParticipantRole};
use serde::{Deserialize, Serialize};
use sqlx::{Row, Sqlite, Transaction};

use crate::{PersistenceError, RoomManagerAuthority, RoomMutationAuthority, SqliteStore};

pub(crate) const DDL: &str = "CREATE TABLE IF NOT EXISTS ai_owner_bindings (room_id TEXT NOT NULL, subject_kind TEXT NOT NULL CHECK(subject_kind IN ('participant','attendee','connector')), subject_id TEXT NOT NULL, binding_json TEXT NOT NULL, PRIMARY KEY(room_id,subject_kind,subject_id), FOREIGN KEY(room_id) REFERENCES rooms(room_id) ON DELETE CASCADE) STRICT";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OwnerBinding {
    room_uid: uuid::Uuid,
    user_id: String,
    participant_id: String,
    reference: OwnerReference,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
enum OwnerReference {
    Local {
        server_id: String,
        authority_lineage_id: String,
    },
    Human {
        fingerprint: [u8; 32],
    },
    Operator {
        fingerprint: [u8; 32],
        device: [u8; 32],
        origin: String,
    },
    ServerOwner {
        fingerprint: [u8; 32],
        device: [u8; 32],
        origin: String,
    },
}

impl OwnerBinding {
    pub(crate) async fn from_manager(
        tx: &mut Transaction<'_, Sqlite>,
        authority: &RoomManagerAuthority,
    ) -> Result<Self, PersistenceError> {
        match authority {
            RoomManagerAuthority::Local(local) => {
                let principal = local.resolve(tx).await?;
                Self::from_mutation(tx, RoomMutationAuthority::TrustedPrincipal(&principal)).await
            }
            RoomManagerAuthority::Operator(operator) => {
                Self::from_mutation(tx, RoomMutationAuthority::OperatorSession(operator)).await
            }
        }
    }

    pub(crate) async fn from_mutation(
        tx: &mut Transaction<'_, Sqlite>,
        authority: RoomMutationAuthority<'_>,
    ) -> Result<Self, PersistenceError> {
        // Issuers resolve this exact authority in the same creation transaction.
        let principal = authority.principal();
        let (room, _) = crate::authority::load_active_membership(
            tx,
            &principal.room_id,
            &principal.participant_id,
        )
        .await?;
        crate::room_user_identity::resolve_room_user_identity(
            tx,
            &principal.room_id,
            &principal.principal_id,
            &principal.participant_id,
        )
        .await?;
        let local = async |tx: &mut Transaction<'_, Sqlite>| {
            let (binding, _) = crate::room_user_identity::resolve_local_room_manager(
                tx,
                &principal.room_id,
                &principal.principal_id,
                &principal.participant_id,
            )
            .await?;
            Ok::<_, PersistenceError>(OwnerReference::Local {
                server_id: binding.server_id,
                authority_lineage_id: binding.authority_lineage_id,
            })
        };
        let operator = |session: &crate::OperatorSessionAuthorization| OwnerReference::Operator {
            fingerprint: *session.session_fingerprint(),
            device: *session.delegation_device(),
            origin: session.delegation_origin().to_owned(),
        };
        let reference = match authority {
            RoomMutationAuthority::TrustedPrincipal(_) => local(tx).await?,
            RoomMutationAuthority::HumanSession(session) => OwnerReference::Human {
                fingerprint: *session.session_fingerprint(),
            },
            RoomMutationAuthority::OperatorSession(session) => operator(session),
            RoomMutationAuthority::ServerOwnerLifecycle(owner) => match owner.owner() {
                crate::ServerOwnerAuthority::LocalOperator => local(tx).await?,
                crate::ServerOwnerAuthority::CentralOwner(session) => operator(session),
                crate::ServerOwnerAuthority::CentralSession(session) => {
                    OwnerReference::ServerOwner {
                        fingerprint: *session.fingerprint(),
                        device: session.binding().browser_fingerprint,
                        origin: session.binding().origin.clone(),
                    }
                }
            },
            RoomMutationAuthority::ConnectorSession(_) => return Err(unavailable()),
        };
        Ok(Self {
            room_uid: room.room_uid,
            user_id: principal.principal_id.clone(),
            participant_id: principal.participant_id.clone(),
            reference,
        })
    }

    pub(crate) async fn save(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        room_id: &str,
        kind: &str,
        id: &str,
    ) -> Result<(), PersistenceError> {
        sqlx::query("INSERT INTO ai_owner_bindings(room_id,subject_kind,subject_id,binding_json) VALUES(?,?,?,?)")
            .bind(room_id).bind(kind).bind(id).bind(serde_json::to_string(self)?)
            .execute(&mut **tx).await?;
        Ok(())
    }

    async fn resolve(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        room_id: &str,
    ) -> Result<AuthenticatedPrincipal, PersistenceError> {
        let room = crate::authority::load_active_room(tx, room_id).await?;
        if room.room_uid != self.room_uid {
            return Err(unavailable());
        }
        let principal = match &self.reference {
            OwnerReference::Local {
                server_id,
                authority_lineage_id,
            } => {
                let expected = crate::LocalRoomManagerAuthority {
                    server_id: server_id.clone(),
                    authority_lineage_id: authority_lineage_id.clone(),
                    room_uid: self.room_uid,
                    manager: crate::RoomUserIdentity {
                        room_id: room_id.to_owned(),
                        user_id: self.user_id.clone(),
                        participant_id: self.participant_id.clone(),
                    },
                };
                expected.resolve(tx).await?
            }
            OwnerReference::Human { fingerprint } => {
                match crate::human_session_authority::resolve_human_session(
                    tx,
                    fingerprint,
                    Some(room_id),
                    chrono::Utc::now(),
                )
                .await?
                {
                    crate::human_session_authority::ResolvedHumanSession::Live {
                        authorization,
                        ..
                    } => authorization.principal().clone(),
                    _ => return Err(unavailable()),
                }
            }
            OwnerReference::Operator {
                fingerprint,
                device,
                origin,
            } => crate::operator_pairing::resolve_operator_session(
                tx,
                fingerprint,
                device,
                origin,
                chrono::Utc::now(),
            )
            .await?
            .principal()
            .clone(),
            OwnerReference::ServerOwner {
                fingerprint,
                device,
                origin,
            } => {
                crate::host_owner_session::resolve(tx, fingerprint, device, origin).await?;
                crate::room_user_identity::resolve_local_room_manager(
                    tx,
                    room_id,
                    &self.user_id,
                    &self.participant_id,
                )
                .await?
                .1
            }
        };
        if principal.room_id != room_id
            || principal.principal_id != self.user_id
            || principal.participant_id != self.participant_id
        {
            return Err(unavailable());
        }
        Ok(principal)
    }

    /// Maps the proven owner reference to the owner's own room-manager authority: the
    /// exact authority the owner's own HTTP invite path builds from its one-use local
    /// control ticket or central-owner session. Only local-bootstrap lineage or a live
    /// central-owner operator session can produce it; a human guest/member, an ordinary
    /// paired operator browser or a server-owner
    /// provenance refuses `owner_authority_unavailable` rather than inventing authority.
    /// # Errors
    /// Rejects a changed room incarnation, unprovable provenance and storage failures.
    pub(crate) async fn manager_authority(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        room_id: &str,
    ) -> Result<RoomManagerAuthority, PersistenceError> {
        let room = crate::authority::load_active_room(tx, room_id).await?;
        if room.room_uid != self.room_uid {
            return Err(unavailable());
        }
        match &self.reference {
            OwnerReference::Local {
                server_id,
                authority_lineage_id,
            } => {
                let manager = crate::LocalRoomManagerAuthority {
                    server_id: server_id.clone(),
                    authority_lineage_id: authority_lineage_id.clone(),
                    room_uid: self.room_uid,
                    manager: crate::RoomUserIdentity {
                        room_id: room_id.to_owned(),
                        user_id: self.user_id.clone(),
                        participant_id: self.participant_id.clone(),
                    },
                };
                let principal = manager.resolve(tx).await?;
                if principal.room_id != room_id
                    || principal.principal_id != self.user_id
                    || principal.participant_id != self.participant_id
                {
                    return Err(unavailable());
                }
                Ok(RoomManagerAuthority::Local(manager))
            }
            OwnerReference::Operator {
                fingerprint,
                device,
                origin,
            } => {
                let session = crate::operator_pairing::resolve_operator_session(
                    tx,
                    fingerprint,
                    device,
                    origin,
                    chrono::Utc::now(),
                )
                .await?;
                // An ordinary paired operator browser resolves to the same
                // local-operator participant but holds no invite-management
                // authority; only central-owner provenance maps to the owner's
                // own room-manager authority.
                if !session.is_central_owner() {
                    return Err(unavailable());
                }
                if session.principal().room_id != room_id
                    || session.principal().principal_id != self.user_id
                    || session.principal().participant_id != self.participant_id
                {
                    return Err(unavailable());
                }
                Ok(RoomManagerAuthority::Operator(Box::new(session)))
            }
            OwnerReference::Human { .. } | OwnerReference::ServerOwner { .. } => Err(unavailable()),
        }
    }
}

/// Resolves the exact room-manager authority owned by the bound owner of one delegate,
/// or refuses. A companion delegate's provenance lives on its invitation and is never a
/// manager authority, so it refuses here.
/// # Errors
/// Refuses `owner_authority_unavailable` for unprovable or ineligible provenance.
pub(crate) async fn manager_authority_for_delegate(
    tx: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    delegate_id: &str,
) -> Result<RoomManagerAuthority, PersistenceError> {
    let companion: i64 = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM room_attendee_invites WHERE room_id=? AND participant_id=? AND parent_fingerprint IS NOT NULL)")
        .bind(room_id)
        .bind(delegate_id)
        .fetch_one(&mut **tx)
        .await?;
    if companion != 0 {
        return Err(unavailable());
    }
    let encoded: Option<String> = sqlx::query_scalar("SELECT binding_json FROM ai_owner_bindings WHERE room_id=? AND subject_kind='participant' AND subject_id=?")
        .bind(room_id)
        .bind(delegate_id)
        .fetch_optional(&mut **tx)
        .await?;
    let binding: OwnerBinding = serde_json::from_str(&encoded.ok_or_else(unavailable)?)?;
    binding.manager_authority(tx, room_id).await
}

// Immutable non-bearer owner reference and its current authority deadline.
pub(crate) async fn confirmation_provenance(
    tx: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    delegate_id: &str,
) -> Result<(serde_json::Value, Option<chrono::DateTime<chrono::Utc>>), PersistenceError> {
    if let Some(row) = sqlx::query("SELECT parent_fingerprint,creator_id,owner_participant_id,room_uid FROM room_attendee_invites WHERE room_id=? AND participant_id=? AND parent_fingerprint IS NOT NULL")
        .bind(room_id).bind(delegate_id).fetch_optional(&mut **tx).await? {
        let fingerprint: Vec<u8> = row.get("parent_fingerprint");
        let (_, expiry) = crate::attendee::session::resolve_parent(tx, room_id, &fingerprint, chrono::Utc::now()).await?;
        return Ok((serde_json::json!({"parent_fingerprint":fingerprint,
            "creator_id":row.get::<String,_>("creator_id"),
            "owner_participant_id":row.get::<String,_>("owner_participant_id"),
            "room_uid":row.get::<String,_>("room_uid")}), expiry));
    }
    let encoded: String = sqlx::query_scalar("SELECT binding_json FROM ai_owner_bindings WHERE room_id=? AND subject_kind='participant' AND subject_id=?")
        .bind(room_id).bind(delegate_id).fetch_optional(&mut **tx).await?.ok_or_else(unavailable)?;
    let binding: OwnerBinding = serde_json::from_str(&encoded)?;
    let expiry = match &binding.reference {
        OwnerReference::Local { .. } | OwnerReference::ServerOwner { .. } => None,
        OwnerReference::Human { fingerprint } => {
            let crate::human_session_authority::ResolvedHumanSession::Live {
                authorization, ..
            } = crate::human_session_authority::resolve_human_session(
                tx,
                fingerprint,
                Some(room_id),
                chrono::Utc::now(),
            )
            .await?
            else {
                return Err(unavailable());
            };
            Some(authorization.expires_at())
        }
        OwnerReference::Operator {
            fingerprint,
            device,
            origin,
        } => crate::operator_pairing::resolve_operator_session(
            tx,
            fingerprint,
            device,
            origin,
            chrono::Utc::now(),
        )
        .await?
        .expires_at(),
    };
    Ok((serde_json::to_value(binding)?, expiry))
}

// The invitation owns the reference until admission. Missing historical references stay missing.
pub(crate) async fn admit_binding(
    tx: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    kind: &str,
    invite_id: &str,
    participant_id: &str,
) -> Result<(), PersistenceError> {
    sqlx::query("UPDATE ai_owner_bindings SET subject_kind='participant',subject_id=? WHERE room_id=? AND subject_kind=? AND subject_id=?")
        .bind(participant_id).bind(room_id).bind(kind).bind(invite_id).execute(&mut **tx).await?;
    Ok(())
}

pub(crate) async fn resolve_pin_owner(
    tx: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    delegate_id: &str,
    allow_detached: bool,
) -> Result<AuthenticatedPrincipal, PersistenceError> {
    let delegate = load_delegate_member(tx, room_id, delegate_id, allow_detached).await?;
    if delegate.participant_type != "agent" || delegate.role != ParticipantRole::Director {
        return Err(rejected("role_required", "A director AI is required."));
    }
    if delegate.muted {
        return Err(rejected(
            "delegate_authority_expired",
            "The delegate is muted.",
        ));
    }
    let owner = resolve_bound_owner(tx, room_id, delegate_id)
        .await
        .map_err(|error| match error {
            PersistenceError::CommandRejected { .. }
            | PersistenceError::ParticipantMissing
            | PersistenceError::RoomMissing => unavailable(),
            other => other,
        })?;
    if owner.participant_id != delegate.owner_id {
        return Err(unavailable());
    }
    crate::message_pins::require_pin_authority(tx, &owner)
        .await
        .map_err(|error| match error {
            PersistenceError::CommandRejected { code, .. } if code == "participant_muted" => {
                rejected("owner_muted", "The owner is muted.")
            }
            PersistenceError::CommandRejected { code, .. } if code == "permission_denied" => {
                rejected("owner_scope_read_only", "The owner cannot pin messages.")
            }
            other => other,
        })?;
    Ok(owner)
}

/// Loads a delegate participant for delegation authority. Membership intact
/// (`joined`, or `detached` when the caller tolerates runtime absence) proves the
/// delegate; removal, disconnect of the membership itself and revoked actors refuse
/// exactly as the shared active-membership gate refuses them.
pub(crate) async fn load_delegate_member(
    tx: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    delegate_id: &str,
    allow_detached: bool,
) -> Result<agentsassemble_domain::Participant, PersistenceError> {
    crate::authority::load_active_room(tx, room_id).await?;
    let participant = crate::participant_rows::load_participant_by_key(tx, room_id, delegate_id)
        .await?
        .ok_or(PersistenceError::ParticipantMissing)?;
    let attached = participant.room_id == room_id
        && participant.participant_id == delegate_id
        && (participant.status == agentsassemble_domain::ParticipantStatus::Joined
            || (allow_detached
                && participant.status == agentsassemble_domain::ParticipantStatus::Detached));
    if !attached {
        return Err(crate::authority::session_revoked());
    }
    crate::central_member_removal::require_live_actor(tx, delegate_id).await?;
    Ok(participant)
}

async fn resolve_bound_owner(
    tx: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    delegate_id: &str,
) -> Result<AuthenticatedPrincipal, PersistenceError> {
    // Companion provenance already lives on its invitation; do not duplicate or infer it.
    if let Some(row) = sqlx::query("SELECT parent_fingerprint,creator_id,owner_participant_id,room_uid FROM room_attendee_invites WHERE room_id=? AND participant_id=? AND parent_fingerprint IS NOT NULL")
        .bind(room_id).bind(delegate_id).fetch_optional(&mut **tx).await? {
        let room = crate::authority::load_active_room(tx, room_id).await?;
        if room.room_uid.to_string() != row.get::<String,_>("room_uid") { return Err(unavailable()); }
        let (owner, _) = crate::attendee::session::resolve_parent(tx, room_id, row.get("parent_fingerprint"), chrono::Utc::now()).await?;
        if owner.principal_id != row.get::<String,_>("creator_id") || owner.participant_id != row.get::<String,_>("owner_participant_id") { return Err(unavailable()); }
        return Ok(owner);
    }
    let encoded: Option<String> = sqlx::query_scalar("SELECT binding_json FROM ai_owner_bindings WHERE room_id=? AND subject_kind='participant' AND subject_id=?")
        .bind(room_id).bind(delegate_id).fetch_optional(&mut **tx).await?;
    let binding: OwnerBinding = serde_json::from_str(&encoded.ok_or_else(unavailable)?)?;
    binding.resolve(tx, room_id).await
}

impl SqliteStore {
    /// Fresh owner/role eligibility for tool discovery; execution must also prove AI custody.
    /// # Errors
    /// Propagates storage corruption/failure; unavailable authority is not an eligible tool.
    pub async fn director_owner_can_pin(
        &self,
        room_id: &str,
        delegate_id: &str,
    ) -> Result<bool, PersistenceError> {
        let mut tx = self.pool.begin().await?;
        let result = resolve_pin_owner(&mut tx, room_id, delegate_id, false).await;
        tx.commit().await?;
        match result {
            Ok(_) => Ok(true),
            Err(
                PersistenceError::CommandRejected { .. }
                | PersistenceError::ParticipantMissing
                | PersistenceError::RoomMissing,
            ) => Ok(false),
            Err(error) => Err(error),
        }
    }
}

pub(crate) fn unavailable() -> PersistenceError {
    rejected(
        "owner_authority_unavailable",
        "The bound owner's authority is unavailable.",
    )
}
fn rejected(code: &'static str, message: &'static str) -> PersistenceError {
    PersistenceError::CommandRejected {
        code: code.into(),
        message: message.into(),
    }
}

#[cfg(test)]
#[path = "delegation_owner_tests.rs"]
mod tests;
