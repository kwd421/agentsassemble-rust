use std::{io, sync::OnceLock};

use sha2::{Digest, Sha256};
use uuid::Uuid;

const HANDLE_PREFIX: &str = "runtime-v4-";
const BOOT_IDENTITY_BYTES: usize = 64;
static CURRENT_IDENTITY: OnceLock<Result<String, String>> = OnceLock::new();

pub(crate) fn current_identity() -> io::Result<&'static str> {
    CURRENT_IDENTITY
        .get_or_init(|| load_current_identity().map_err(|error| error.to_string()))
        .as_deref()
        .map_err(|error| io::Error::other(error.to_owned()))
}

fn load_current_identity() -> io::Result<String> {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    let source = {
        let value = std::fs::read_to_string("/proc/sys/kernel/random/boot_id")?;
        Uuid::parse_str(value.trim())
            .map_err(|_| io::Error::other("kernel boot identity is invalid"))?
            .to_string()
    };
    #[cfg(target_os = "macos")]
    let source = {
        let boot_time = sysinfo::System::boot_time();
        if boot_time == 0 {
            return Err(io::Error::other("kernel boot identity is unavailable"));
        }
        boot_time.to_string()
    };
    #[cfg(not(any(target_os = "linux", target_os = "android", target_os = "macos")))]
    let source = return Err(io::Error::other(
        "kernel boot identity is unsupported on this platform",
    ));
    let mut digest = Sha256::new();
    digest.update(std::env::consts::OS.as_bytes());
    digest.update([0]);
    digest.update(source.as_bytes());
    Ok(format!("{:x}", digest.finalize()))
}

pub(crate) fn new_handle_id(boot_identity: &str) -> String {
    debug_assert!(is_valid_identity(boot_identity));
    format!("{HANDLE_PREFIX}{boot_identity}-{}", Uuid::new_v4())
}

pub(crate) fn handle_is_from_current_boot(handle_id: &str) -> io::Result<bool> {
    let encoded = handle_id
        .strip_prefix(HANDLE_PREFIX)
        .ok_or_else(|| io::Error::other("provider runtime handle version is invalid"))?;
    let (boot_identity, runtime_id) = encoded
        .split_once('-')
        .ok_or_else(|| io::Error::other("provider runtime handle is invalid"))?;
    if !is_valid_identity(boot_identity) {
        return Err(io::Error::other(
            "provider runtime boot identity is invalid",
        ));
    }
    Uuid::parse_str(runtime_id)
        .map_err(|_| io::Error::other("provider runtime handle identity is invalid"))?;
    Ok(boot_identity == current_identity()?)
}

pub(crate) fn is_valid_identity(identity: &str) -> bool {
    identity.len() == BOOT_IDENTITY_BYTES
        && identity
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
mod tests {
    #[test]
    fn runtime_handle_binds_one_exact_boot_generation() {
        let current_boot = super::current_identity()
            .unwrap_or_else(|error| panic!("read current boot identity: {error}"));
        let current = super::new_handle_id(current_boot);
        assert!(
            super::handle_is_from_current_boot(&current)
                .unwrap_or_else(|error| panic!("classify current boot handle: {error}"))
        );
        let mut previous = current.into_bytes();
        let first_boot_digit = super::HANDLE_PREFIX.len();
        previous[first_boot_digit] = if previous[first_boot_digit] == b'0' {
            b'1'
        } else {
            b'0'
        };
        let previous = String::from_utf8(previous)
            .unwrap_or_else(|error| panic!("encode prior boot handle: {error}"));
        assert!(
            !super::handle_is_from_current_boot(&previous)
                .unwrap_or_else(|error| panic!("classify previous boot handle: {error}"))
        );
    }
}
