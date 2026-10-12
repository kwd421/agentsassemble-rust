//! Every durable table has one explicit backup classification. Schema growth
//! fails export until its authority and reconstruction policy are classified.
use crate::{
    PersistenceError,
    backup_json::{self, invalid, strict},
};
use agentsassemble_domain::{
    DurableAgentSession, Participant, PersonaCard, Room, RoomSettings, RoomUserPreferences,
    SavedFriend, UserProfile,
};
use serde_json::Value;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Policy {
    Product,
    Ephemeral,
    Metadata,
}

pub(crate) fn policy(table: &str) -> Result<Policy, PersistenceError> {
    Ok(match table {
        "rooms"
        | "participants"
        | "user_profiles"
        | "persona_assets"
        | "profile_avatar_assets"
        | "room_user_preferences"
        | "room_invites"
        | "google_accounts"
        | "saved_friends"
        | "room_appearance_assets"
        | "agent_sessions"
        | "agent_avatar_assets"
        | "room_events"
        | "room_message_attachments"
        | "room_message_pins"
        | "room_turn_tool_results"
        | "room_vote_states"
        | "room_vote_ballots"
        | "command_results"
        | "room_create_results"
        | "room_delete_results"
        | "room_channel_retirements"
        | "central_identity_bindings"
        | "central_member_removals"
        | "member_admissions"
        | "room_attendee_invites"
        | "room_connector_invites"
        | "local_bootstrap_authority" => Policy::Product,
        "central_owner_sessions"
        | "host_owner_sessions"
        | "central_owner_grants"
        | "operator_pairings"
        | "human_device_credentials"
        | "guest_recovery_codes"
        | "human_room_sessions"
        | "attendee_connections"
        | "attendee_cleanup_controls"
        | "room_connector_uploads"
        | "connector_mentions"
        | "prejoin_avatar_assets"
        | "provider_requests"
        | "provider_turn_executions"
        | "provider_turn_effects"
        | "lifecycle_command_reservations"
        | "ordered_input_routes"
        | "room_runtime_cleanup"
        | "member_projection_outbox"
        | "member_projection_sender"
        | "room_message_search_records"
        | "room_message_search_phrase"
        | "room_write_budgets"
        | "room_event_publication_cursors"
        | "runtime_host_identity"
        | "runtime_host_initialization" => Policy::Ephemeral,
        "runtime_metadata" => Policy::Metadata,
        _ => return Err(invalid()),
    })
}

fn decode<T: serde::de::DeserializeOwned + serde::Serialize>(
    value: Value,
) -> Result<Value, PersistenceError> {
    Ok(serde_json::to_value(strict::<T>(value)?)?)
}

pub(crate) fn json(table: &str, column: &str, encoded: &str) -> Result<String, PersistenceError> {
    let value: Value = serde_json::from_str(encoded).map_err(|_| invalid())?;
    let decoded = match (table, column) {
        ("rooms", "room_json") => decode::<Room>(value)?,
        ("rooms", "settings_json") => decode::<RoomSettings>(value)?,
        ("participants", "participant_json") => decode::<Participant>(value)?,
        ("user_profiles", "profile_json") => decode::<UserProfile>(value)?,
        ("persona_assets", "card_json") => decode::<PersonaCard>(value)?,
        ("room_user_preferences", "preferences_json") => decode::<RoomUserPreferences>(value)?,
        ("saved_friends", "friend_json") => decode::<SavedFriend>(value)?,
        ("agent_sessions", "session_json") => {
            let mut session: DurableAgentSession = strict(value)?;
            backup_json::stop_agent(&mut session);
            serde_json::to_value(session)?
        }
        ("room_events", "event_json") => serde_json::to_value(backup_json::event(value)?)?,
        ("member_admissions", "result_json") => backup_json::admission(value)?,
        ("command_results" | "room_create_results" | "room_delete_results", "result_json") => {
            backup_json::receipt(value)?
        }
        ("local_bootstrap_authority", "result_json") => {
            decode::<crate::LocalBootstrapCommit>(value)?
        }
        ("room_vote_states", "tallies_json") => decode::<Vec<u64>>(value)?,
        _ => return Err(invalid()),
    };
    Ok(serde_json::to_string(&decoded)?)
}
