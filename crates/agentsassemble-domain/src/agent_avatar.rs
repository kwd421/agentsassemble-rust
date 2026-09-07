pub const AGENT_AVATAR_ID_PREFIX: &str = "aa_";
pub const AGENT_AVATAR_HEX_LENGTH: usize = 32;
pub const AGENT_AVATAR_REFERENCE_PREFIX: &str = "/api/agent-avatars/";

#[must_use]
pub fn is_agent_avatar_asset_id(id: &str) -> bool {
    id.strip_prefix(AGENT_AVATAR_ID_PREFIX)
        .is_some_and(|suffix| {
            suffix.len() == AGENT_AVATAR_HEX_LENGTH
                && suffix
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
}

#[must_use]
pub fn agent_avatar_asset_id(reference: &str) -> Option<&str> {
    reference
        .strip_prefix(AGENT_AVATAR_REFERENCE_PREFIX)
        .filter(|id| is_agent_avatar_asset_id(id))
}

#[must_use]
pub fn agent_avatar_url(id: &str) -> Option<String> {
    is_agent_avatar_asset_id(id).then(|| format!("{AGENT_AVATAR_REFERENCE_PREFIX}{id}"))
}
