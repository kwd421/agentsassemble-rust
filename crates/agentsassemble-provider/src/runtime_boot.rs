use std::{io, sync::OnceLock};

use sha2::{Digest, Sha256};
use uuid::Uuid;

const HANDLE_PREFIX: &str = "runtime-v5-";
const BOOT_IDENTITY_BYTES: usize = 64;
const UUID_TEXT_BYTES: usize = 36;
static CURRENT_IDENTITY: OnceLock<Result<String, String>> = OnceLock::new();

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RuntimeHandleIdentity {
    pub(crate) boot_identity: String,
    pub(crate) launch_token: String,
}

pub(crate) fn current_identity() -> io::Result<&'static str> {
    CURRENT_IDENTITY
        .get_or_init(|| load_current_identity().map_err(|error| error.to_string()))
        .as_deref()
        .map_err(|error| io::Error::other(error.to_owned()))
}

fn load_current_identity() -> io::Result<String> {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    let source = std::fs::read_to_string("/proc/sys/kernel/random/boot_id")?;
    #[cfg(target_os = "macos")]
    let source = macos_boot_session_uuid_source()?;
    #[cfg(not(any(target_os = "linux", target_os = "android", target_os = "macos")))]
    let source = return Err(io::Error::other(
        "kernel boot identity is unsupported on this platform",
    ));
    boot_identity_from_uuid(std::env::consts::OS, source.trim())
}

fn boot_identity_from_uuid(platform: &str, source: &str) -> io::Result<String> {
    let canonical = canonical_uuid(source, "kernel boot identity is invalid")?;
    Ok(hash_boot_identity(platform, &canonical))
}

fn hash_boot_identity(platform: &str, source: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(platform.as_bytes());
    digest.update([0]);
    digest.update(source.as_bytes());
    format!("{:x}", digest.finalize())
}

#[cfg(target_os = "macos")]
fn macos_boot_session_uuid_source() -> io::Result<String> {
    use sysctl::Sysctl;

    let control = sysctl::Ctl::new("kern.bootsessionuuid")
        .map_err(|_| io::Error::other("kernel boot identity is unavailable"))?;
    control
        .value_string()
        .map_err(|_| io::Error::other("kernel boot identity is unavailable"))
}

fn canonical_uuid(value: &str, error: &'static str) -> io::Result<String> {
    let parsed = Uuid::parse_str(value).map_err(|_| io::Error::other(error))?;
    Ok(parsed.to_string())
}

pub(crate) fn new_handle_id(boot_identity: &str, launch_token: &str) -> String {
    debug_assert!(is_valid_identity(boot_identity));
    debug_assert!(is_canonical_uuid(launch_token));
    format!(
        "{HANDLE_PREFIX}{boot_identity}-{launch_token}-{}",
        Uuid::new_v4()
    )
}

pub(crate) fn parse_handle_id(handle_id: &str) -> io::Result<RuntimeHandleIdentity> {
    let encoded = handle_id
        .strip_prefix(HANDLE_PREFIX)
        .ok_or_else(|| io::Error::other("provider runtime handle version is invalid"))?;
    let boot_identity = encoded
        .get(..BOOT_IDENTITY_BYTES)
        .ok_or_else(|| io::Error::other("provider runtime handle is invalid"))?;
    if !is_valid_identity(boot_identity) {
        return Err(io::Error::other(
            "provider runtime boot identity is invalid",
        ));
    }
    let suffix = encoded
        .get(BOOT_IDENTITY_BYTES..)
        .and_then(|value| value.strip_prefix('-'))
        .ok_or_else(|| io::Error::other("provider runtime handle is invalid"))?;
    let launch_token = suffix
        .get(..UUID_TEXT_BYTES)
        .ok_or_else(|| io::Error::other("provider runtime handle identity is invalid"))?;
    let runtime_id = suffix
        .get(UUID_TEXT_BYTES..)
        .and_then(|value| value.strip_prefix('-'))
        .ok_or_else(|| io::Error::other("provider runtime handle identity is invalid"))?;
    if !is_canonical_uuid(launch_token) || !is_canonical_uuid(runtime_id) {
        return Err(io::Error::other(
            "provider runtime handle identity is invalid",
        ));
    }
    Ok(RuntimeHandleIdentity {
        boot_identity: boot_identity.to_owned(),
        launch_token: launch_token.to_owned(),
    })
}

pub(crate) fn is_valid_identity(identity: &str) -> bool {
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
    #[test]
    fn runtime_handle_binds_one_exact_boot_and_launch_generation() {
        let current_boot = super::current_identity()
            .unwrap_or_else(|error| panic!("read current boot identity: {error}"));
        let launch_token = uuid::Uuid::new_v4().to_string();
        let current = super::new_handle_id(current_boot, &launch_token);
        let parsed = super::parse_handle_id(&current)
            .unwrap_or_else(|error| panic!("parse current boot handle: {error}"));
        assert_eq!(parsed.boot_identity, current_boot);
        assert_eq!(parsed.launch_token, launch_token);
        let mut previous = current.clone().into_bytes();
        let first_boot_digit = super::HANDLE_PREFIX.len();
        previous[first_boot_digit] = if previous[first_boot_digit] == b'0' {
            b'1'
        } else {
            b'0'
        };
        let previous = String::from_utf8(previous)
            .unwrap_or_else(|error| panic!("encode prior boot handle: {error}"));
        let previous = super::parse_handle_id(&previous)
            .unwrap_or_else(|error| panic!("parse previous boot handle: {error}"));
        assert_ne!(previous.boot_identity, current_boot);
        assert_eq!(previous.launch_token, launch_token);
    }

    #[test]
    fn mac_boot_identity_depends_only_on_the_immutable_boot_session_uuid() {
        let source = "00000000-0000-4000-8000-00000000000A";
        let first = super::boot_identity_from_uuid("macos", source)
            .unwrap_or_else(|error| panic!("derive boot session identity: {error}"));
        let second = super::boot_identity_from_uuid("macos", &source.to_ascii_lowercase())
            .unwrap_or_else(|error| panic!("derive lowercase boot session identity: {error}"));
        assert_eq!(
            first, second,
            "wall-clock state is not an input to boot-session identity"
        );
        assert!(super::boot_identity_from_uuid("macos", "1700000000").is_err());
    }
}
