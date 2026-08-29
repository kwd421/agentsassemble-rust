pub const MESSAGE_ATTACHMENT_ID_PREFIX: &str = "ma_";
pub const MESSAGE_ATTACHMENT_ID_HEX_LENGTH: usize = 32;
pub const MAX_MESSAGE_ATTACHMENTS_PER_EVENT: usize = 8;

/// Reports whether one opaque identifier belongs to the message-attachment namespace.
#[must_use]
pub fn is_message_attachment_id(attachment_id: &str) -> bool {
    let Some(hex) = attachment_id.strip_prefix(MESSAGE_ATTACHMENT_ID_PREFIX) else {
        return false;
    };
    hex.len() == MESSAGE_ATTACHMENT_ID_HEX_LENGTH
        && hex
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
mod tests {
    use super::is_message_attachment_id;

    #[test]
    fn message_attachment_namespace_is_exact_lowercase_hex() {
        assert!(is_message_attachment_id(
            "ma_0123456789abcdef0123456789abcdef"
        ));
        for rejected in [
            "0123456789abcdef0123456789abcdef",
            "ma_0123456789abcdef0123456789abcde",
            "ma_0123456789abcdef0123456789abcdeg",
            "ma_0123456789ABCDEF0123456789ABCDEF",
            "ma_0123456789abcdef0123456789abcdef\0",
        ] {
            assert!(!is_message_attachment_id(rejected), "accepted {rejected:?}");
        }
    }
}
