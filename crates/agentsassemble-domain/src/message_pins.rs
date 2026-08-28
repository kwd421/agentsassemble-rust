pub const MAX_LOBBY_MESSAGE_PINS: i64 = 64;
pub const MAX_MESSAGE_PIN_EVENT_ID_BYTES: usize = 128;

#[must_use]
pub fn is_message_pin_event_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= MAX_MESSAGE_PIN_EVENT_ID_BYTES && !value.contains('\0')
}

#[cfg(test)]
mod tests {
    use super::{MAX_MESSAGE_PIN_EVENT_ID_BYTES, is_message_pin_event_id};

    #[test]
    fn pin_event_ids_use_utf8_bytes_and_reject_nul() {
        assert!(is_message_pin_event_id("event-1"));
        assert!(is_message_pin_event_id(
            &"x".repeat(MAX_MESSAGE_PIN_EVENT_ID_BYTES)
        ));
        assert!(is_message_pin_event_id(
            &"é".repeat(MAX_MESSAGE_PIN_EVENT_ID_BYTES / 2)
        ));
        assert!(!is_message_pin_event_id(""));
        assert!(!is_message_pin_event_id(
            &"x".repeat(MAX_MESSAGE_PIN_EVENT_ID_BYTES + 1)
        ));
        assert!(!is_message_pin_event_id(
            &"é".repeat(MAX_MESSAGE_PIN_EVENT_ID_BYTES / 2 + 1)
        ));
        assert!(!is_message_pin_event_id("event\0tail"));
    }
}
