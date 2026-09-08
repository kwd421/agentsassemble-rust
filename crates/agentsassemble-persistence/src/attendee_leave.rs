use agentsassemble_domain::{ParticipantStatus, canonical_payload_hash};
use chrono::Utc;
use serde_json::json;
use uuid::Uuid;

use crate::{
    AttendeeCleanupAuthorization, CommandOutcome, PersistenceError, RoomCommandMutation,
    SqliteStore,
    agent_lifecycle::{load_participant, load_session},
    attendee_cleanup::revalidate_in,
    attendee_invites::rejected,
    command_admission::{inspect_non_lifecycle_command, store_command_result},
    participant_leave::participant_left_event,
    participant_removal::revoke_participant_access,
    participant_rows::save_participant_exact,
    room_runtime_cleanup::request_runtime_cleanup,
    room_turns::support::{insert_event, session_state_event},
};

impl SqliteStore {
    /// Ends only the authenticated external owner's membership and requests exact cleanup.
    /// Expiry ends ordinary room access without preventing this self-disabling operation.
    ///
    /// # Errors
    /// Rejects changed custody, conflicting retry identities and already removed membership.
    pub async fn leave_attendee(
        &self,
        authority: &AttendeeCleanupAuthorization,
        request_id: Uuid,
    ) -> Result<RoomCommandMutation, PersistenceError> {
        if request_id.is_nil() {
            return Err(rejected("bad_request", "A leave request UUID is required."));
        }
        let request = request_id.to_string();
        let action = "bridge.leave";
        let hash = canonical_payload_hash(&json!({}));
        let key = &authority.key;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        revalidate_in(&mut tx, authority).await?;
        if let Some(outcome) = inspect_non_lifecycle_command(
            &mut tx,
            &key.room_id,
            &key.session_id,
            &request,
            action,
            &hash,
        )
        .await?
        {
            tx.commit().await?;
            return Ok(RoomCommandMutation {
                outcome,
                assignments: Vec::new(),
            });
        }
        let mut participant = load_participant(&mut tx, &key.room_id, &key.session_id).await?;
        if participant.status != ParticipantStatus::Joined {
            return Err(rejected(
                "session_revoked",
                "This attendee membership has ended.",
            ));
        }
        let mut session = load_session(&mut tx, &key.room_id, &key.session_id).await?;
        request_runtime_cleanup(&mut tx, &mut session).await?;
        revoke_participant_access(&mut tx, &key.room_id, &key.session_id).await?;
        participant.status = ParticipantStatus::Left;
        participant.updated_at = Utc::now();
        save_participant_exact(&mut tx, &key.room_id, &key.session_id, &participant).await?;
        let state = session_state_event(&mut tx, &session).await?;
        let event = participant_left_event(&mut tx, &participant).await?;
        insert_event(&mut tx, &event).await?;
        let events = vec![state, event.clone()];
        let result = json!({"event":event, "events":events, "cleanup_pending":true});
        store_command_result(
            &mut tx,
            (&key.room_id, &key.session_id),
            &request,
            action,
            &hash,
            &result,
        )
        .await?;
        tx.commit().await?;
        Ok(RoomCommandMutation {
            outcome: CommandOutcome {
                result,
                event,
                events,
                deduplicated: false,
            },
            assignments: Vec::new(),
        })
    }
}
