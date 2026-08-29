use sha2::{Digest, Sha256};

#[must_use]
pub fn runtime_profile_key(fields: [&str; 14]) -> String {
    format!(
        "provider-profile-v2-{:x}",
        Sha256::digest(fields.join("\0").as_bytes())
    )
}
