/// Per-item limit for existing uploads, avatars, personas and provider reads.
pub const MAX_ATTACHMENT_BYTES: usize = 10 * 1024 * 1024;

/// Retained room-file ceiling, independent of the current upload policy.
pub const MAX_ROOM_FILE_BYTES: usize = 50 * 1024 * 1024;

/// Supported room policy presets, in binary bytes.
pub const ROOM_FILE_LIMITS: [usize; 3] =
    [MAX_ATTACHMENT_BYTES, 25 * 1024 * 1024, MAX_ROOM_FILE_BYTES];
pub const ROOM_STORAGE_LIMITS: [u64; 5] = [
    500 * 1024 * 1024,
    1024 * 1024 * 1024,
    2 * 1024 * 1024 * 1024,
    4 * 1024 * 1024 * 1024,
    8 * 1024 * 1024 * 1024,
];

pub(crate) fn default_file_limit() -> usize {
    MAX_ATTACHMENT_BYTES
}
pub(crate) fn is_default_file_limit(value: &usize) -> bool {
    *value == MAX_ATTACHMENT_BYTES
}
pub(crate) fn deserialize_file_limit<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<usize, D::Error> {
    let value = <usize as serde::Deserialize>::deserialize(d)?;
    if ROOM_FILE_LIMITS.contains(&value) {
        Ok(value)
    } else {
        Err(serde::de::Error::custom(
            "Unsupported attachment_file_limit_bytes.",
        ))
    }
}
pub(crate) fn deserialize_storage_limit<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<Option<u64>, D::Error> {
    let value = <Option<u64> as serde::Deserialize>::deserialize(d)?;
    if value.is_none_or(|v| ROOM_STORAGE_LIMITS.contains(&v)) {
        Ok(value)
    } else {
        Err(serde::de::Error::custom(
            "Unsupported attachment_storage_limit_bytes.",
        ))
    }
}
