use std::{
    hash::{Hash, Hasher},
    io::{self, Read},
};

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

/// Binds an already-open readable object's stable filesystem identity to its bytes.
///
/// # Errors
///
/// Returns the reader error without producing a partial identity.
pub fn stable_content_identity(
    identity: &(impl Hash + ?Sized),
    mut content: impl Read,
) -> io::Result<String> {
    let mut digest = Sha256::new();
    digest.update(b"agentsassemble-content-identity-v1\0");
    digest.update(stable_identity_hash(identity).as_bytes());
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        let count = content.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(format!("content-identity-v1-{:x}", digest.finalize()))
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

#[cfg(test)]
mod tests {
    use super::{stable_bundle_identity, stable_content_identity, stable_identity_hash};

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

    #[test]
    fn content_identity_binds_object_and_bytes() {
        let first = stable_content_identity(&1_u64, b"provider bytes".as_slice())
            .unwrap_or_else(|error| panic!("hash content: {error}"));
        let changed_bytes = stable_content_identity(&1_u64, b"changed bytes".as_slice())
            .unwrap_or_else(|error| panic!("hash changed content: {error}"));
        let changed_object = stable_content_identity(&2_u64, b"provider bytes".as_slice())
            .unwrap_or_else(|error| panic!("hash changed object: {error}"));
        assert_ne!(first, changed_bytes);
        assert_ne!(first, changed_object);
    }

    #[test]
    fn bundle_identity_binds_kind_order_and_every_member() {
        let first = stable_bundle_identity("codex-native", &["main", "host"]);
        assert_eq!(
            first,
            stable_bundle_identity("codex-native", &["main", "host"])
        );
        assert_ne!(
            first,
            stable_bundle_identity("codex-native", &["host", "main"])
        );
        assert_ne!(first, stable_bundle_identity("other", &["main", "host"]));
    }
}
