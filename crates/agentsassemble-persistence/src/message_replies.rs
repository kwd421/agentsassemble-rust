//! Reply identity belongs to the message transaction, never to a copied quote.
use crate::{PersistenceError, room_turns::support::load_event};

pub(crate) async fn validate_reply_target(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    room_id: &str,
    channel_id: &str,
    target_id: Option<&str>,
    before_seq: i64,
) -> Result<Option<String>, PersistenceError> {
    let Some(id) = target_id else {
        return Ok(None);
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
    // The replied-to author, whom an "@ on" reply calls.
    Ok(Some(target.actor.participant_id))
}

/// Records whom a reply calls with Discord's "@ on". Replying to yourself
/// calls nobody.
pub(crate) fn record_reply_mention(
    event: &mut agentsassemble_domain::RoomEvent,
    reply_author: Option<String>,
    mention: bool,
) {
    if let Some(author) =
        reply_author.filter(|author| mention && *author != event.actor.participant_id)
    {
        event.extra.insert(
            agentsassemble_domain::REPLY_MENTION_FIELD.to_owned(),
            serde_json::json!(author),
        );
    }
}

/// An AI's text in Discord's reply shape. A mention wrapped in escaped
/// brackets (`&lt;<@ID>&gt;`, a model habit) becomes a plain `<@ID>`. A reply
/// that opens by mentioning the replied-to author calls them with "@ on"
/// instead, so the text carries no `@` (returns the text and whether it calls).
pub(crate) fn agent_reply_text(content: &str, reply_author: Option<&str>) -> (String, bool) {
    let mut text = String::with_capacity(content.len());
    let mut rest = content;
    while let Some(start) = rest.find("&lt;<@") {
        let after = &rest[start + "&lt;".len()..];
        match after
            .find('>')
            .filter(|end| after[end + 1..].starts_with("&gt;"))
        {
            Some(end) => {
                text.push_str(&rest[..start]);
                text.push_str(&after[..=end]);
                rest = &after[end + 1 + "&gt;".len()..];
            }
            None => {
                text.push_str(&rest[..start + "&lt;".len()]);
                rest = after;
            }
        }
    }
    text.push_str(rest);
    let Some(author) = reply_author else {
        return (text, false);
    };
    let lead = format!("<@{author}>");
    let remainder = text.trim_start().strip_prefix(lead.as_str()).map(|rest| {
        rest.trim_start_matches(|character: char| {
            character.is_whitespace() || matches!(character, ',' | ':')
        })
    });
    match remainder {
        Some(remainder) if agentsassemble_domain::has_visible_text(remainder) => {
            (remainder.to_owned(), true)
        }
        _ => (text, false),
    }
}

fn rejected(code: &'static str, message: &str) -> PersistenceError {
    PersistenceError::CommandRejected {
        code: code.into(),
        message: message.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::agent_reply_text;

    #[test]
    fn escaped_brackets_around_a_mention_are_dropped() {
        let (text, calls) = agent_reply_text("&lt;<@operator-local>&gt; 확인했어요", None);
        assert_eq!(text, "<@operator-local> 확인했어요");
        assert!(!calls);
        // Escaped brackets that wrap no mention stay as written.
        assert_eq!(
            agent_reply_text("a &lt; b &gt; c", None).0,
            "a &lt; b &gt; c"
        );
    }

    #[test]
    fn a_reply_opening_with_its_author_calls_them_instead_of_showing_the_mention() {
        let (text, calls) = agent_reply_text(
            "&lt;<@operator-local>&gt;, 확인했어요",
            Some("operator-local"),
        );
        assert_eq!(text, "확인했어요");
        assert!(calls);
        // A mention of someone else, or of the author mid-sentence, stays text.
        let (text, calls) = agent_reply_text("<@other> 확인했어요", Some("operator-local"));
        assert_eq!(text, "<@other> 확인했어요");
        assert!(!calls);
        let (text, calls) = agent_reply_text("확인 <@operator-local>", Some("operator-local"));
        assert_eq!(text, "확인 <@operator-local>");
        assert!(!calls);
        // A message that is only the mention keeps it rather than going empty.
        assert_eq!(
            agent_reply_text("<@operator-local>", Some("operator-local")),
            ("<@operator-local>".to_owned(), false)
        );
    }
}
