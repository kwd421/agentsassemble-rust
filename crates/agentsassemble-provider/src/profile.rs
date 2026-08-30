use sha2::{Digest, Sha256};

#[must_use]
pub fn runtime_profile_key(fields: [&str; 14]) -> String {
    format!(
        "provider-profile-v2-{:x}",
        Sha256::digest(fields.join("\0").as_bytes())
    )
}

#[must_use]
pub(crate) fn runtime_profile_key_with_output(
    fields: [&str; 14],
    max_output_tokens: u32,
) -> String {
    if max_output_tokens == 0 {
        return runtime_profile_key(fields);
    }
    let mut hasher = Sha256::new();
    hasher.update(fields.join("\0"));
    hasher.update([0]);
    hasher.update(max_output_tokens.to_string());
    format!("provider-profile-v2-{:x}", hasher.finalize())
}
