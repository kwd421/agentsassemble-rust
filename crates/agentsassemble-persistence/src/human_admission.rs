use agentsassemble_domain::{avatar_attachment_id, canonical_avatar_url, clean_identifier};
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

use crate::HumanInviteCredentialEvidence;

const ONE_USE_KEY_CONTEXT: &[u8] = b"agentsassemble-human-admission-key-one-use-v1\0";
const REUSABLE_KEY_CONTEXT: &[u8] = b"agentsassemble-human-admission-key-reusable-v1\0";
const PAYLOAD_CONTEXT: &[u8] = b"agentsassemble-human-admission-payload-v1\0";
const AVATAR_CUSTODY_CONTEXT: &[u8] = b"agentsassemble-human-prejoin-avatar-custody-v1\0";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum HumanAdmissionInputError {
    #[error("human admission request ID must be one canonical nonzero UUID")]
    RequestId,
    #[error("browser invite admission requires a human participant type")]
    ParticipantType,
}

/// Raw-free browser admission fields before durable invite authority is consulted.
///
/// This type deliberately implements neither `Debug` nor serialization so browser
/// and invite fingerprints cannot enter generic diagnostics or wire projections.
pub struct PreparedHumanAdmission {
    credential: HumanInviteCredentialEvidence,
    browser_credential_fingerprint: [u8; 32],
    request_id: Uuid,
    meeting_id_assertion: String,
    display_name: String,
    participant_type_input: String,
    owner_display_name: String,
    client_id: String,
    avatar_attachment_id: Option<String>,
}

/// Bounded browser inputs accepted by the current direct HTTP admission contract.
///
/// The transport must authenticate raw invite/browser credentials separately. This
/// value contains no credential or bearer and deliberately is not serializable.
pub struct HumanAdmissionInput {
    pub request_id: String,
    pub meeting_id_assertion: String,
    pub display_name: String,
    pub participant_type: String,
    pub owner_display_name: String,
    pub client_id: String,
    pub avatar_image_url: String,
}

impl PreparedHumanAdmission {
    /// Canonicalizes the bounded request without reading or mutating persistence.
    ///
    /// # Errors
    ///
    /// Rejects a noncanonical/nil request UUID or a participant type that does not
    /// resolve to the human-only browser admission class.
    pub fn prepare(
        credential: HumanInviteCredentialEvidence,
        browser_credential_fingerprint: [u8; 32],
        input: &HumanAdmissionInput,
    ) -> Result<Self, HumanAdmissionInputError> {
        let request_id = canonical_request_id(&input.request_id)?;
        let participant_type_input = clean_identifier(&input.participant_type, 32);
        if !is_human_participant_type(&participant_type_input) {
            return Err(HumanAdmissionInputError::ParticipantType);
        }
        let avatar_attachment_id = canonical_avatar_url(&input.avatar_image_url)
            .and_then(|url| avatar_attachment_id(&url).map(str::to_owned));
        Ok(Self {
            credential,
            browser_credential_fingerprint,
            request_id,
            meeting_id_assertion: clean_identifier(&input.meeting_id_assertion, 128),
            display_name: clean_identifier(&input.display_name, 128),
            participant_type_input,
            owner_display_name: clean_identifier(&input.owner_display_name, 64),
            client_id: clean_identifier(&input.client_id, 128),
            avatar_attachment_id,
        })
    }

    #[must_use]
    pub fn credential(&self) -> &HumanInviteCredentialEvidence {
        &self.credential
    }

    #[must_use]
    pub const fn browser_credential_fingerprint(&self) -> &[u8; 32] {
        &self.browser_credential_fingerprint
    }

    #[must_use]
    pub const fn request_id(&self) -> Uuid {
        self.request_id
    }

    #[must_use]
    pub fn meeting_id_assertion(&self) -> &str {
        &self.meeting_id_assertion
    }

    #[must_use]
    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    #[must_use]
    pub fn participant_type_input(&self) -> &str {
        &self.participant_type_input
    }

    #[must_use]
    pub fn owner_display_name(&self) -> &str {
        &self.owner_display_name
    }

    #[must_use]
    pub fn client_id(&self) -> &str {
        &self.client_id
    }

    #[must_use]
    pub fn avatar_attachment_id(&self) -> Option<&str> {
        self.avatar_attachment_id.as_deref()
    }

    #[must_use]
    pub fn one_use_admission_key(&self) -> [u8; 32] {
        let mut digest = Sha256::new();
        digest.update(ONE_USE_KEY_CONTEXT);
        digest.update(self.credential.fingerprint());
        digest.update(self.browser_credential_fingerprint);
        digest.update(self.request_id.as_bytes());
        digest.finalize().into()
    }

    #[must_use]
    pub fn reusable_admission_key(&self) -> [u8; 32] {
        let mut digest = Sha256::new();
        digest.update(REUSABLE_KEY_CONTEXT);
        digest.update(self.credential.fingerprint());
        digest.update(self.browser_credential_fingerprint);
        digest.finalize().into()
    }

    #[must_use]
    pub fn payload_hash(&self) -> [u8; 32] {
        let mut digest = Sha256::new();
        digest.update(PAYLOAD_CONTEXT);
        update_field(&mut digest, &self.meeting_id_assertion);
        update_field(&mut digest, &self.display_name);
        update_field(&mut digest, &self.participant_type_input);
        update_field(&mut digest, &self.owner_display_name);
        update_field(&mut digest, &self.client_id);
        if let Some(attachment_id) = &self.avatar_attachment_id {
            digest.update([1]);
            update_field(&mut digest, attachment_id);
        } else {
            digest.update([0]);
        }
        digest.finalize().into()
    }

    #[must_use]
    pub fn avatar_custody_fingerprint(&self) -> [u8; 32] {
        let mut digest = Sha256::new();
        digest.update(AVATAR_CUSTODY_CONTEXT);
        digest.update(self.credential.fingerprint());
        digest.update(self.browser_credential_fingerprint);
        digest.finalize().into()
    }
}

impl HumanInviteCredentialEvidence {
    #[must_use]
    pub const fn fingerprint(&self) -> &[u8; 32] {
        match self {
            Self::Signed { fingerprint, .. } | Self::JoinCode { fingerprint } => fingerprint,
        }
    }
}

fn canonical_request_id(value: &str) -> Result<Uuid, HumanAdmissionInputError> {
    let value = value.trim();
    let request_id = Uuid::parse_str(value).map_err(|_| HumanAdmissionInputError::RequestId)?;
    if request_id.is_nil() || request_id.hyphenated().to_string() != value {
        return Err(HumanAdmissionInputError::RequestId);
    }
    Ok(request_id)
}

fn is_human_participant_type(value: &str) -> bool {
    matches!(
        value.to_ascii_lowercase().as_str(),
        "" | "human" | "person" | "user"
    )
}

fn update_field(digest: &mut Sha256, value: &str) {
    let bytes = value.as_bytes();
    let length = u32::try_from(bytes.len())
        .unwrap_or_else(|_| unreachable!("canonical admission fields are bounded"));
    digest.update(length.to_be_bytes());
    digest.update(bytes);
}

#[cfg(test)]
#[path = "human_admission_tests.rs"]
mod tests;
