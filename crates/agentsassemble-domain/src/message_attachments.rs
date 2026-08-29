pub const MESSAGE_ATTACHMENT_ID_PREFIX: &str = "ma_";
pub const MESSAGE_ATTACHMENT_ID_HEX_LENGTH: usize = 32;
pub const MAX_MESSAGE_ATTACHMENTS_PER_EVENT: usize = 8;
pub const MAX_MESSAGE_ATTACHMENT_FILENAME_CHARACTERS: usize = 120;
pub const MAX_MESSAGE_ATTACHMENT_CONTENT_TYPE_BYTES: usize = 127;
pub const MESSAGE_ATTACHMENT_REFERENCE_PREFIX: &str = "/api/attachments/";
pub const MESSAGE_ATTACHMENT_VIEW_SUFFIX: &str = "?view=1";
pub const MESSAGE_ATTACHMENT_DOWNLOAD_SUFFIX: &str = "?download=1";

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

/// Returns the canonical safe display and download name for a message attachment.
#[must_use]
pub fn canonical_message_attachment_filename(value: &str) -> String {
    let normalized = value.replace('\\', "/");
    let name = normalized.rsplit('/').next().unwrap_or_default();
    let truncated: String = name
        .chars()
        .filter(|character| !character.is_control() && !matches!(character, '/' | '\\'))
        .collect::<String>()
        .trim()
        .chars()
        .take(MAX_MESSAGE_ATTACHMENT_FILENAME_CHARACTERS)
        .collect();
    let name = truncated.trim();
    if name.is_empty() || matches!(name, "." | "..") {
        "attachment.bin".to_owned()
    } else {
        name.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::{canonical_message_attachment_filename, is_message_attachment_id};

    #[test]
    fn message_attachment_filename_discards_paths_and_unsafe_names() {
        assert_eq!(
            canonical_message_attachment_filename("../folder/evidence.txt"),
            "evidence.txt"
        );
        assert_eq!(
            canonical_message_attachment_filename(".."),
            "attachment.bin"
        );
        assert_eq!(
            canonical_message_attachment_filename("\0"),
            "attachment.bin"
        );
    }

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
