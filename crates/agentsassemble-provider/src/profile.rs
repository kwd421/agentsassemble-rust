use sha2::{Digest, Sha256};

pub(crate) fn runtime_profile_key(fields: [&str; 13]) -> String {
    format!(
        "provider-profile-v1-{:x}",
        Sha256::digest(fields.join("\0").as_bytes())
    )
}
