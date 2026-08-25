use std::io;

use uuid::Uuid;

const UNIX_HANDLE_PREFIX: &str = "runtime-v5-";
const WINDOWS_HANDLE_PREFIX: &str = "runtime-v5-windows-";
const BOOT_IDENTITY_BYTES: usize = 64;
const UUID_TEXT_BYTES: usize = 36;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimeHandlePlatform {
    Unix,
    Windows,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RuntimeHandleIdentity {
    pub(crate) platform: RuntimeHandlePlatform,
    pub(crate) boot_identity: Option<String>,
    pub(crate) launch_token: String,
}

#[cfg(any(unix, test))]
pub(crate) fn new_unix_handle_id(boot_identity: &str, launch_token: &str) -> String {
    debug_assert!(is_valid_boot_identity(boot_identity));
    debug_assert!(is_canonical_uuid(launch_token));
    format!(
        "{UNIX_HANDLE_PREFIX}{boot_identity}-{launch_token}-{}",
        Uuid::new_v4()
    )
}

pub(crate) fn parse_handle_id(handle_id: &str) -> io::Result<RuntimeHandleIdentity> {
    if let Some(encoded) = handle_id.strip_prefix(WINDOWS_HANDLE_PREFIX) {
        return parse_windows_handle(encoded);
    }
    let encoded = handle_id
        .strip_prefix(UNIX_HANDLE_PREFIX)
        .ok_or_else(|| io::Error::other("provider runtime handle version is invalid"))?;
    parse_unix_handle(encoded)
}

fn parse_windows_handle(encoded: &str) -> io::Result<RuntimeHandleIdentity> {
    let (launch_token, _runtime_id) = parse_uuid_pair(encoded)?;
    Ok(RuntimeHandleIdentity {
        platform: RuntimeHandlePlatform::Windows,
        boot_identity: None,
        launch_token: launch_token.to_owned(),
    })
}

fn parse_unix_handle(encoded: &str) -> io::Result<RuntimeHandleIdentity> {
    let boot_identity = encoded
        .get(..BOOT_IDENTITY_BYTES)
        .filter(|identity| is_valid_boot_identity(identity))
        .ok_or_else(|| io::Error::other("provider runtime boot identity is invalid"))?;
    let suffix = encoded
        .get(BOOT_IDENTITY_BYTES..)
        .and_then(|value| value.strip_prefix('-'))
        .ok_or_else(|| io::Error::other("provider runtime handle is invalid"))?;
    let (launch_token, _runtime_id) = parse_uuid_pair(suffix)?;
    Ok(RuntimeHandleIdentity {
        platform: RuntimeHandlePlatform::Unix,
        boot_identity: Some(boot_identity.to_owned()),
        launch_token: launch_token.to_owned(),
    })
}

fn parse_uuid_pair(encoded: &str) -> io::Result<(&str, &str)> {
    let launch_token = encoded
        .get(..UUID_TEXT_BYTES)
        .ok_or_else(|| io::Error::other("provider runtime handle identity is invalid"))?;
    let runtime_id = encoded
        .get(UUID_TEXT_BYTES..)
        .and_then(|value| value.strip_prefix('-'))
        .ok_or_else(|| io::Error::other("provider runtime handle identity is invalid"))?;
    if !is_canonical_uuid(launch_token) || !is_canonical_uuid(runtime_id) {
        return Err(io::Error::other(
            "provider runtime handle identity is invalid",
        ));
    }
    Ok((launch_token, runtime_id))
}

pub(crate) fn is_valid_boot_identity(identity: &str) -> bool {
    identity.len() == BOOT_IDENTITY_BYTES
        && identity
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn is_canonical_uuid(value: &str) -> bool {
    Uuid::parse_str(value).is_ok_and(|parsed| parsed.to_string() == value)
}

#[cfg(test)]
mod tests {
    use super::{RuntimeHandlePlatform, parse_handle_id};

    #[test]
    fn strict_decoder_distinguishes_unix_and_windows_generations() {
        let token = uuid::Uuid::new_v4().to_string();
        let unix = super::new_unix_handle_id(&"a".repeat(64), &token);
        let windows = format!("runtime-v5-windows-{token}-{}", uuid::Uuid::new_v4());

        let unix = parse_handle_id(&unix)
            .unwrap_or_else(|error| panic!("parse Unix runtime handle: {error}"));
        assert_eq!(unix.platform, RuntimeHandlePlatform::Unix);
        let expected_boot = "a".repeat(64);
        assert_eq!(unix.boot_identity.as_deref(), Some(expected_boot.as_str()));
        assert_eq!(unix.launch_token, token);

        let windows = parse_handle_id(&windows)
            .unwrap_or_else(|error| panic!("parse Windows runtime handle: {error}"));
        assert_eq!(windows.platform, RuntimeHandlePlatform::Windows);
        assert_eq!(windows.boot_identity, None);
        assert_eq!(windows.launch_token, token);
    }

    #[test]
    fn decoder_rejects_cross_version_and_trailing_data() {
        let token = uuid::Uuid::new_v4().to_string();
        let runtime = uuid::Uuid::new_v4();
        assert!(
            parse_handle_id(&format!("runtime-v4-{}-{token}-{runtime}", "a".repeat(64))).is_err()
        );
        assert!(parse_handle_id(&format!("runtime-v5-windows-{token}-{runtime}-extra")).is_err());
    }
}
