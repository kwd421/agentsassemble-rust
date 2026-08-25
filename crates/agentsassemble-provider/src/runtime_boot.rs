use std::{io, sync::OnceLock};

use sha2::{Digest, Sha256};
use uuid::Uuid;

static CURRENT_IDENTITY: OnceLock<Result<String, String>> = OnceLock::new();

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

#[cfg(test)]
mod tests {
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
