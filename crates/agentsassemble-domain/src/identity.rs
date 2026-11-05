use std::{
    hash::{Hash, Hasher},
    io::{self, Read},
};

use same_file::Handle;
use sha2::{Digest, Sha256};

struct StableIdentityHasher(Sha256);

impl StableIdentityHasher {
    fn write_integer(&mut self, bytes: &[u8]) {
        self.0
            .update(u64::try_from(bytes.len()).unwrap_or(u64::MAX).to_le_bytes());
        self.0.update(bytes);
    }
}

impl Hasher for StableIdentityHasher {
    fn finish(&self) -> u64 {
        let digest = self.0.clone().finalize();
        u64::from_le_bytes(digest[..8].try_into().unwrap_or([0; 8]))
    }

    fn write(&mut self, bytes: &[u8]) {
        self.0.update((bytes.len() as u64).to_le_bytes());
        self.0.update(bytes);
    }

    fn write_u8(&mut self, value: u8) {
        self.write_integer(&value.to_le_bytes());
    }

    fn write_u16(&mut self, value: u16) {
        self.write_integer(&value.to_le_bytes());
    }

    fn write_u32(&mut self, value: u32) {
        self.write_integer(&value.to_le_bytes());
    }

    fn write_u64(&mut self, value: u64) {
        self.write_integer(&value.to_le_bytes());
    }

    fn write_u128(&mut self, value: u128) {
        self.write_integer(&value.to_le_bytes());
    }

    fn write_usize(&mut self, value: usize) {
        self.write_u64(value as u64);
    }

    fn write_i8(&mut self, value: i8) {
        self.write_integer(&value.to_le_bytes());
    }

    fn write_i16(&mut self, value: i16) {
        self.write_integer(&value.to_le_bytes());
    }

    fn write_i32(&mut self, value: i32) {
        self.write_integer(&value.to_le_bytes());
    }

    fn write_i64(&mut self, value: i64) {
        self.write_integer(&value.to_le_bytes());
    }

    fn write_i128(&mut self, value: i128) {
        self.write_integer(&value.to_le_bytes());
    }

    fn write_isize(&mut self, value: isize) {
        self.write_i64(value as i64);
    }
}

#[must_use]
pub fn stable_identity_hash(value: &(impl Hash + ?Sized)) -> String {
    let mut hasher = StableIdentityHasher(Sha256::new());
    value.hash(&mut hasher);
    format!("identity-v1-{:x}", hasher.0.finalize())
}

/// The part of an open object's identity that survives a reboot. macOS renumbers
/// Unix device numbers at boot, so Unix keeps only the inode; every caller also
/// compares the exact canonical path, which already fixes the volume. Windows
/// volume serials and file indexes are stable, so the whole handle is kept.
struct RebootStableKey<'a>(&'a Handle);

impl Hash for RebootStableKey<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        #[cfg(unix)]
        self.0.ino().hash(state);
        #[cfg(not(unix))]
        self.0.hash(state);
    }
}

/// Identifies an open filesystem object across reboots.
#[must_use]
pub fn stable_file_identity(handle: &Handle) -> String {
    let mut hasher = StableIdentityHasher(Sha256::new());
    RebootStableKey(handle).hash(&mut hasher);
    format!("file-identity-v2-{:x}", hasher.0.finalize())
}

/// Binds an already-open readable object's reboot-stable identity to its bytes.
///
/// # Errors
///
/// Returns the reader error without producing a partial identity.
pub fn stable_content_identity(handle: &Handle, mut content: impl Read) -> io::Result<String> {
    let mut digest = Sha256::new();
    digest.update(b"agentsassemble-content-identity-v2\0");
    digest.update(stable_file_identity(handle).as_bytes());
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        let count = content.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(format!("content-identity-v2-{:x}", digest.finalize()))
}

#[must_use]
pub fn stable_bundle_identity(bundle_kind: &str, members: &[&str]) -> String {
    let mut digest = Sha256::new();
    digest.update(b"agentsassemble-bundle-identity-v1\0");
    for value in std::iter::once(bundle_kind).chain(members.iter().copied()) {
        digest.update(u64::try_from(value.len()).unwrap_or(u64::MAX).to_le_bytes());
        digest.update(value.as_bytes());
    }
    format!("bundle-identity-v1-{:x}", digest.finalize())
}

#[must_use]
pub fn codex_bundle_identity(executable_identity: &str, companion_identity: &str) -> String {
    stable_bundle_identity("codex-native", &[executable_identity, companion_identity])
}

#[must_use]
pub const fn codex_code_mode_host_name() -> &'static str {
    if cfg!(windows) {
        "codex-code-mode-host.exe"
    } else {
        "codex-code-mode-host"
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use same_file::Handle;

    use super::{
        stable_bundle_identity, stable_content_identity, stable_file_identity, stable_identity_hash,
    };

    #[test]
    fn stable_identity_separates_values() {
        assert_eq!(
            stable_identity_hash(&(1_u64, 2_u64)),
            stable_identity_hash(&(1_u64, 2_u64))
        );
        assert_ne!(
            stable_identity_hash(&(1_u64, 2_u64)),
            stable_identity_hash(&(2_u64, 1_u64))
        );
    }

    fn handle(name: &str) -> Handle {
        Handle::from_path(Path::new(env!("CARGO_MANIFEST_DIR")).join(name))
            .unwrap_or_else(|error| panic!("open {name}: {error}"))
    }

    #[test]
    fn content_identity_binds_object_and_bytes() {
        let manifest = handle("Cargo.toml");
        let first = stable_content_identity(&manifest, b"provider bytes".as_slice())
            .unwrap_or_else(|error| panic!("hash content: {error}"));
        let changed_bytes = stable_content_identity(&manifest, b"changed bytes".as_slice())
            .unwrap_or_else(|error| panic!("hash changed content: {error}"));
        let changed_object = stable_content_identity(&handle("src"), b"provider bytes".as_slice())
            .unwrap_or_else(|error| panic!("hash changed object: {error}"));
        assert_ne!(first, changed_bytes);
        assert_ne!(first, changed_object);
    }

    #[test]
    fn file_identity_follows_the_object_not_the_open() {
        assert_eq!(
            stable_file_identity(&handle("Cargo.toml")),
            stable_file_identity(&handle("Cargo.toml"))
        );
        assert_ne!(
            stable_file_identity(&handle("Cargo.toml")),
            stable_file_identity(&handle("src"))
        );
    }

    /// macOS renumbers device numbers at boot, so the identity must not change
    /// when only the device number does.
    #[cfg(unix)]
    #[test]
    fn file_identity_ignores_the_unix_device_number() {
        let manifest = handle("Cargo.toml");
        let mut hasher = super::StableIdentityHasher(<sha2::Sha256 as sha2::Digest>::new());
        std::hash::Hash::hash(&manifest.ino(), &mut hasher);
        let inode_only = format!("file-identity-v2-{:x}", sha2::Digest::finalize(hasher.0));
        assert_eq!(stable_file_identity(&manifest), inode_only);
    }

    #[test]
    fn bundle_identity_binds_kind_order_and_every_member() {
        let first = stable_bundle_identity("bundle-kind", &["main", "host"]);
        assert_eq!(
            first,
            stable_bundle_identity("bundle-kind", &["main", "host"])
        );
        assert_ne!(
            first,
            stable_bundle_identity("bundle-kind", &["host", "main"])
        );
        assert_ne!(first, stable_bundle_identity("other", &["main", "host"]));
    }
}
