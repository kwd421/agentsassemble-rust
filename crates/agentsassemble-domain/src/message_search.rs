use caseless::default_case_fold_str;
use serde::{Deserialize, Serialize};

use crate::{
    RoomEvent,
    text::{clean_trimmed_crlf_text, is_python_whitespace},
};

pub const MESSAGE_SEARCH_PAGE_SIZE: usize = 30;
pub const MESSAGE_CONTEXT_RADIUS: usize = 15;
pub const MAX_MESSAGE_SEARCH_QUERY_CHARACTERS: usize = 200;
pub const MAX_MESSAGE_SEARCH_CURSOR_BYTES: usize = 2_048;
pub const MAX_MESSAGE_SEARCH_AUTHOR_CHARACTERS: usize = 128;
pub const MAX_MESSAGE_SEARCH_CONTENT_CHARACTERS: usize = 12_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LobbyMessageSearchResult {
    pub event_id: String,
    pub participant_id: String,
    pub seq: i64,
    pub created_at: String,
    pub author: String,
    pub content: String,
    pub attachment_filenames: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LobbyMessageSearchPage {
    pub results: Vec<LobbyMessageSearchResult>,
    pub next_cursor: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LobbyMessageContext {
    pub event_id: String,
    pub events: Vec<RoomEvent>,
}

#[must_use]
pub fn clean_message_search_query(value: &str) -> String {
    clean_message_search_value(value, MAX_MESSAGE_SEARCH_QUERY_CHARACTERS)
}

#[must_use]
pub fn clean_message_search_value(value: &str, limit: usize) -> String {
    clean_trimmed_crlf_text(value, limit)
}

#[must_use]
pub fn casefold_message_search_text(value: &str) -> String {
    default_case_fold_str(value)
}

#[must_use]
pub fn compact_casefolded_message_search_text(value: &str) -> String {
    value
        .chars()
        .filter(|character| !is_python_whitespace(*character))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        casefold_message_search_text, clean_message_search_query,
        compact_casefolded_message_search_text,
    };

    #[test]
    fn query_preserves_original_single_line_limit() {
        assert_eq!(
            clean_message_search_query("  old\r\nmessage  "),
            "old  message"
        );
        assert_eq!(
            clean_message_search_query(&format!("{} tail", "가".repeat(200))),
            "가".repeat(200)
        );
        assert_eq!(
            clean_message_search_query(&format!("{}old", "\u{001c}".repeat(200))),
            "old"
        );
    }

    #[test]
    fn compact_search_uses_full_casefold_and_python_whitespace() {
        let folded = casefold_message_search_text("Straße\u{001c} 배포\t오류");
        assert_eq!(
            compact_casefolded_message_search_text(&folded),
            "strasse배포오류"
        );
    }
}
