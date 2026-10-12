/// Per-item limit for existing uploads, avatars, personas and provider reads.
pub const MAX_ATTACHMENT_BYTES: usize = 10 * 1024 * 1024;

/// Retained room-file ceiling, independent of the current upload policy.
pub const MAX_ROOM_FILE_BYTES: usize = 50 * 1024 * 1024;
