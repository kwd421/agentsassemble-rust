use agentsassemble_domain::{LOCAL_OPERATOR_USER_ID, Participant, ParticipantStatus, RoomEvent};
use chrono::Utc;
use sqlx::{Row, Sqlite, Transaction};

use crate::{
    AccountUser, PersistenceError,
    account_identity::{device_user_id, rejected},
    google_accounts::account_fingerprint,
    participant_leave::participant_left_event,
    participant_removal::revoke_participant_access,
    participant_rows::save_participant_exact,
    room_turns::support::insert_event,
};

pub(crate) async fn retire_guest(
    transaction: &mut Transaction<'_, Sqlite>,
    guest: &AccountUser,
    device: Option<&[u8; 32]>,
) -> Result<(Vec<RoomEvent>, Vec<[u8; 32]>), PersistenceError> {
    // The current Rust room owner is the bootstrapped local operator; it cannot be retired.
    if guest.user_id == LOCAL_OPERATOR_USER_ID {
        return Err(rejected(
            "account_switch_operator_forbidden",
            "The server operator cannot be discarded.",
        ));
    }
    let device = device.ok_or_else(|| {
        rejected(
            "account_switch_unavailable",
            "A guest device is required for account switching.",
        )
    })?;
    if account_fingerprint(transaction, &guest.user_id)
        .await?
        .is_some()
        || device_user_id(transaction, device).await?.as_deref() != Some(&guest.user_id)
    {
        return Err(rejected(
            "account_switch_unavailable",
            "The device must still belong to an unlinked guest.",
        ));
    }
    let rows = sqlx::query("SELECT room_id, participant_json FROM participants WHERE participant_id = ? ORDER BY room_id").bind(&guest.participant_id).fetch_all(&mut **transaction).await?;
    let mut events = Vec::new();
    let mut revoked = Vec::new();
    for row in rows {
        let room_id: String = row.try_get("room_id")?;
        let mut participant: Participant = serde_json::from_str(row.try_get("participant_json")?)?;
        if participant.room_id != room_id
            || participant.participant_id != guest.participant_id
            || participant.participant_type != "human"
        {
            return Err(rejected(
                "invalid_state",
                "Guest membership binding is invalid.",
            ));
        }
        revoked
            .extend(revoke_participant_access(transaction, &room_id, &guest.participant_id).await?);
        if participant.status == ParticipantStatus::Joined {
            participant.status = ParticipantStatus::Left;
            participant.updated_at = Utc::now();
            save_participant_exact(transaction, &room_id, &guest.participant_id, &participant)
                .await?;
            let event = participant_left_event(transaction, &participant).await?;
            insert_event(transaction, &event).await?;
            events.push(event);
        }
    }
    sqlx::query("DELETE FROM user_profiles WHERE user_id = ?")
        .bind(&guest.user_id)
        .execute(&mut **transaction)
        .await?;
    Ok((events, revoked))
}
