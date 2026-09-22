//! Reply identity belongs to the message transaction, never to a copied quote.
use crate::{PersistenceError, room_turns::support::load_event};

pub(crate) async fn validate_reply_target(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    room_id: &str,
    channel_id: &str,
    target_id: Option<&str>,
    before_seq: i64,
) -> Result<(), PersistenceError> {
    let Some(id) = target_id else {
        return Ok(());
    };
    let target = load_event(tx, room_id, id).await?.ok_or_else(|| {
        rejected(
            "reply_target_unavailable",
            "The reply target is unavailable in this channel.",
        )
    })?;
    let same_channel = if channel_id == "lobby" {
        target.event_type == "message_final"
    } else {
        target.event_type == "channel_message_final"
            && target
                .extra
                .get("channel_id")
                .and_then(serde_json::Value::as_str)
                == Some(channel_id)
    };
    if !same_channel
        || target.seq >= before_seq
        || target
            .extra
            .get("message_deleted")
            .and_then(serde_json::Value::as_bool)
            == Some(true)
        || !matches!(target.message_kind.as_deref(), Some("message" | "vote"))
    {
        return Err(rejected(
            "reply_target_unavailable",
            "The reply target is unavailable in this channel.",
        ));
    }
    Ok(())
}

fn rejected(code: &'static str, message: &str) -> PersistenceError {
    PersistenceError::CommandRejected {
        code: code.into(),
        message: message.to_owned(),
    }
}
