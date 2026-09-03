use crate::has_visible_text;

pub const MAX_PROVIDER_INPUT_CHARACTERS: usize = 20_000;
pub const MAX_PROVIDER_TURN_ID_BYTES: usize = 128;
pub const MAX_ROOM_OBSERVATION_AGENT_IDS: usize = 64;
pub const MAX_ROOM_OBSERVATION_AGENT_ID_BYTES: usize = 128;
pub const MAX_ROOM_VIEW_BYTES: usize = 96 * 1024;
pub const MAX_ROOM_VIEW_CHARACTERS: usize = 20_000;

#[must_use]
pub fn is_provider_input(value: &str) -> bool {
    value.chars().count() <= MAX_PROVIDER_INPUT_CHARACTERS
        && !value.contains('\0')
        && has_visible_text(value)
}

#[must_use]
pub fn is_provider_turn_id(value: &str) -> bool {
    canonical_bounded_id(value, MAX_PROVIDER_TURN_ID_BYTES)
}

#[must_use]
pub fn is_room_observation_agent_id(value: &str) -> bool {
    canonical_bounded_id(value, MAX_ROOM_OBSERVATION_AGENT_ID_BYTES)
}

#[must_use]
pub fn is_room_observation_view(value: &str) -> bool {
    value.chars().count() <= MAX_ROOM_VIEW_CHARACTERS
        && value.len() <= MAX_ROOM_VIEW_BYTES
        && !value.contains('\0')
        && has_visible_text(value)
}

fn canonical_bounded_id(value: &str, max_bytes: usize) -> bool {
    !value.is_empty()
        && value.len() <= max_bytes
        && value.trim() == value
        && !value.chars().any(char::is_control)
}
