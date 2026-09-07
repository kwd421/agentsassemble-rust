const REFERENCE_PREFIX: &str = "/api/agent-avatars/";

#[must_use]
pub fn is_agent_avatar_asset_id(id: &str) -> bool {
    id.strip_prefix("aa_").is_some_and(|suffix| {
        suffix.len() == 32
            && suffix
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

#[must_use]
pub fn agent_avatar_asset_id(reference: &str) -> Option<&str> {
    reference
        .strip_prefix(REFERENCE_PREFIX)
        .filter(|id| is_agent_avatar_asset_id(id))
}

#[must_use]
pub fn agent_avatar_url(id: &str) -> Option<String> {
    is_agent_avatar_asset_id(id).then(|| format!("{REFERENCE_PREFIX}{id}"))
}
