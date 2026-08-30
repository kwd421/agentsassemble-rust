use unicode_general_category::{GeneralCategory, get_general_category};

use crate::CommandRejection;

#[must_use]
pub fn clean_single_line(value: &str, limit: usize) -> String {
    value
        .replace(['\r', '\n', '\t'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(limit)
        .collect()
}

#[must_use]
pub fn clean_identifier(value: &str, limit: usize) -> String {
    clean_trimmed_crlf_text(value, limit)
}

/// Normalizes and validates a room identifier at the transport boundary.
///
/// # Errors
///
/// Returns a rejection for empty, traversal-like, or path-shaped identifiers.
pub fn validate_room_id(value: &str) -> Result<String, CommandRejection> {
    let room_id = clean_identifier(value, 128);
    if room_id.is_empty() || matches!(room_id.as_str(), "." | "..") || room_id.contains(['/', '\\'])
    {
        return Err(CommandRejection::new("bad_request", "room_id is required."));
    }
    Ok(room_id)
}

#[must_use]
pub fn clean_message(value: &str, limit: usize) -> String {
    value
        .replace('\0', "")
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .trim()
        .chars()
        .take(limit)
        .collect()
}

#[must_use]
pub fn has_visible_text(value: &str) -> bool {
    value.chars().any(|character| {
        !character.is_whitespace()
            && !matches!(
                get_general_category(character),
                GeneralCategory::Control | GeneralCategory::Format
            )
    })
}

pub(crate) fn is_python_whitespace(character: char) -> bool {
    character.is_whitespace() || matches!(character, '\u{001C}'..='\u{001F}')
}

pub(crate) fn clean_trimmed_crlf_text(value: &str, limit: usize) -> String {
    value
        .replace(['\r', '\n'], " ")
        .trim_matches(is_python_whitespace)
        .chars()
        .take(limit)
        .collect::<String>()
        .trim_matches(is_python_whitespace)
        .to_owned()
}
