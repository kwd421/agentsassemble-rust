use sha2::{Digest, Sha256};

use crate::DurableAgentSession;

#[must_use]
pub fn runtime_profile_key(fields: [&str; 15]) -> String {
    format!(
        "provider-profile-v2-{:x}",
        Sha256::digest(fields.join("\0").as_bytes())
    )
}

#[must_use]
pub fn runtime_profile_key_with_output(fields: [&str; 15], max_output_tokens: u32) -> String {
    if max_output_tokens == 0 {
        return runtime_profile_key(fields);
    }
    let mut hasher = Sha256::new();
    hasher.update(fields.join("\0"));
    hasher.update([0]);
    hasher.update(max_output_tokens.to_string());
    format!("provider-profile-v2-{:x}", hasher.finalize())
}

/// The profile key a stored session's own fields produce.
#[must_use]
pub fn durable_runtime_profile_key(session: &DurableAgentSession) -> String {
    runtime_profile_key_with_output(
        [
            session.public.provider_kind.as_str(),
            session.public.runtime_kind.as_str(),
            session.executable.as_str(),
            session.executable_identity.as_str(),
            session.workspace.as_str(),
            session.workspace_identity.as_str(),
            session.provider_endpoint.as_str(),
            session.public.model.as_str(),
            session.public.reasoning_effort.as_str(),
            session.public.service_tier.as_str(),
            session.public.variant.as_str(),
            session.public.execution_harness.as_str(),
            session.public.permission_mode.as_str(),
            session.public.persona_card_id.as_ref(),
            session.public.transport.as_str(),
        ],
        session.public.max_output_tokens,
    )
}
