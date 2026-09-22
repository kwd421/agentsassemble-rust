//! Compact text for the room reads a provider receives.
//!
//! A tool result stays in the provider's own transcript for the rest of its session,
//! so its size is paid again on every later model call. The full records carry
//! identifiers and metadata the model never uses: one context read of a dozen
//! messages rendered as `RoomEvent` JSON came to 16,000 characters. This keeps what
//! the model reads and what it needs to follow up (a result's event ID).

use std::fmt::Write as _;

use agentsassemble_domain::{
    RoomEvent, RoomMessageContext, RoomMessageSearchPage, is_message_attachment_id,
};

/// A search result is a pointer to a message, so its content is a preview.
const RESULT_PREVIEW_CHARS: usize = 300;
/// Context is read in full, but one runaway message still cannot fill the result.
const CONTEXT_MESSAGE_CHARS: usize = 1_200;

pub(crate) fn search_page(page: &RoomMessageSearchPage) -> String {
    if page.results.is_empty() {
        return "No matching messages.".to_owned();
    }
    let mut out = String::new();
    for result in &page.results {
        let _ = write!(
            out,
            "#{} {} [event {}]: {}",
            result.seq,
            result.author,
            result.event_id,
            clip(&single_line(&result.content), RESULT_PREVIEW_CHARS)
        );
        if !result.attachment_filenames.is_empty() {
            let _ = write!(
                out,
                " (attachments: {})",
                result.attachment_filenames.join(", ")
            );
        }
        out.push('\n');
    }
    if !page.next_cursor.is_empty() {
        let _ = writeln!(out, "More results: cursor {}", page.next_cursor);
    }
    out.push_str("Use read_message_context with an event ID to read around a result.");
    out
}

pub(crate) fn message_context(context: &RoomMessageContext) -> String {
    let mut out = String::new();
    for event in context
        .events
        .iter()
        .filter(|event| event.is_current_lobby_message())
    {
        let author = event.display_name.as_deref().unwrap_or("unknown");
        let body = event.content.as_deref().unwrap_or_default();
        let marker = if event.id == context.event_id {
            " <- the result"
        } else {
            ""
        };
        let attachments = attachment_lines(event);
        let body = if body.trim().is_empty() && !attachments.is_empty() {
            "(attachments only)".to_owned()
        } else {
            clip(body.trim(), CONTEXT_MESSAGE_CHARS)
        };
        let _ = writeln!(
            out,
            "#{} {}{} [event {}]: {}",
            event.seq, author, marker, event.id, body
        );
        if let Some(id) = event
            .extra
            .get("reply_to_event_id")
            .and_then(serde_json::Value::as_str)
        {
            let _ = writeln!(out, "  - Reply to event `{id}`");
        }
        for line in attachments {
            let _ = writeln!(out, "{line}");
        }
    }
    if out.is_empty() {
        return "No messages around that result.".to_owned();
    }
    out.truncate(out.trim_end().len());
    out
}

/// The same attachment lines the room view shows, so `read_attachment` can follow them.
fn attachment_lines(event: &RoomEvent) -> Vec<String> {
    let Some(attachments) = event
        .extra
        .get("attachments")
        .and_then(|value| value.as_array())
    else {
        return Vec::new();
    };
    attachments
        .iter()
        .filter_map(|attachment| {
            let id = attachment.get("id")?.as_str()?;
            if !is_message_attachment_id(id) {
                return None;
            }
            Some(format!(
                "  - Attachment `{id}`: {} ({}; {} bytes)",
                attachment.get("filename")?.as_str()?,
                attachment.get("content_type")?.as_str()?,
                attachment.get("size")?.as_u64()?
            ))
        })
        .collect()
}

fn single_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn clip(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_owned();
    }
    let mut clipped: String = text.chars().take(limit).collect();
    clipped.push('…');
    clipped
}

#[cfg(test)]
mod tests {
    use agentsassemble_domain::{Actor, RoomEvent, RoomMessageSearchResult};
    use chrono::Utc;

    use super::*;

    fn message(id: &str, seq: i64, author: &str, content: &str) -> RoomEvent {
        let mut event: RoomEvent = serde_json::from_value(serde_json::json!({
            "v": 1, "id": id, "seq": seq, "created_at": Utc::now(), "room_id": "room",
            "type": "message_final",
            "actor": {"participant_id": "p", "participant_type": "agent"},
            "display_name": author, "content": content,
            "session_id": "a-long-session-identifier", "turn_id": "turn-identifier",
        }))
        .unwrap_or_else(|error| panic!("fixture event: {error}"));
        event.actor = Actor {
            participant_id: "p".to_owned(),
            participant_type: "agent".to_owned(),
        };
        event
    }

    #[test]
    fn a_search_page_keeps_what_a_follow_up_needs_and_drops_the_rest() {
        let long = "word ".repeat(200);
        let text = search_page(&RoomMessageSearchPage {
            results: vec![RoomMessageSearchResult {
                channel_id: "lobby".to_owned(),
                event_id: "message-1".to_owned(),
                participant_id: "participant-with-a-long-identifier".to_owned(),
                seq: 7,
                created_at: "2026-09-21T02:28:03.946483300Z".to_owned(),
                author: "Human".to_owned(),
                content: format!("line one\n\nline two {long}"),
                attachment_filenames: vec!["notes.txt".to_owned()],
            }],
            next_cursor: "cursor-2".to_owned(),
        });

        assert!(
            text.starts_with("#7 Human [event message-1]: line one line two word"),
            "{text}"
        );
        assert!(
            text.contains("…"),
            "a long message is previewed, not copied: {text}"
        );
        assert!(text.contains("(attachments: notes.txt)"));
        assert!(text.contains("More results: cursor cursor-2"));
        assert!(!text.contains("participant-with-a-long-identifier"));
        assert!(!text.contains("2026-09-21T02:28"));
    }

    #[test]
    fn context_reads_as_lines_of_the_messages_themselves() {
        let text = message_context(&RoomMessageContext {
            channel_id: "lobby".to_owned(),
            event_id: "b".to_owned(),
            events: vec![
                message("a", 4, "Human", "rock"),
                message("b", 5, "grok", "  paper  "),
            ],
        });

        assert_eq!(
            text,
            "#4 Human [event a]: rock\n#5 grok <- the result [event b]: paper"
        );
        assert!(!text.contains("a-long-session-identifier"));
    }

    #[test]
    fn context_lists_attachments_so_they_can_be_read() {
        let mut photo = message("c", 6, "Human", "");
        photo.extra.insert(
            "attachments".to_owned(),
            serde_json::json!([{
                "id": "ma_11111111111111111111111111111111", "filename": "cat.png",
                "content_type": "image/png", "size": 42, "is_image": true
            }]),
        );
        let text = message_context(&RoomMessageContext {
            channel_id: "lobby".to_owned(),
            event_id: "c".to_owned(),
            events: vec![photo],
        });

        assert_eq!(
            text,
            "#6 Human <- the result [event c]: (attachments only)
  - Attachment `ma_11111111111111111111111111111111`: cat.png (image/png; 42 bytes)"
        );
    }

    #[test]
    fn empty_reads_say_so() {
        assert_eq!(
            search_page(&RoomMessageSearchPage {
                results: Vec::new(),
                next_cursor: String::new()
            }),
            "No matching messages."
        );
        assert_eq!(
            message_context(&RoomMessageContext {
                channel_id: "lobby".to_owned(),
                event_id: "x".to_owned(),
                events: Vec::new(),
            }),
            "No messages around that result."
        );
    }
}
