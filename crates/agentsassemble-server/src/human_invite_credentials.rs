use std::sync::Arc;

use agentsassemble_domain::InviteScope;
use agentsassemble_persistence::PersistentHostIdentity;
use agentsassemble_protocol::{
    HUMAN_INVITE_JOIN_CODE_BYTES, HUMAN_INVITE_JOIN_CODE_PREFIX, HUMAN_INVITE_SIGNATURE_BYTES,
    HUMAN_INVITE_SIGNED_TOKEN_MAX_BYTES, HUMAN_INVITE_SIGNED_TOKEN_PREFIX,
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, SecondsFormat, Utc};
use hmac::{Hmac, Mac};
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use thiserror::Error;
use url::{Host, Url};

use crate::ingress_trust::is_loopback_http_host;

const CLAIM_SCHEMA: &str = "agentsassemble.lan_invite.v1";
const CLAIM_MODE: &str = "lan_invite_token";
const CLAIM_CLIENT_KIND: &str = "native_remote_room_client";
const HUMAN_PROVIDER_KIND: &str = "manual";
const SIGNED_NONCE_BYTES: usize = 18;
const MAX_PUBLIC_ROOM_URL_CHARS: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum HumanInviteCredentialError {
    #[error("human invite credential is missing")]
    Missing,
    #[error("human invite credential is malformed")]
    Malformed,
    #[error("human invite credential signature is invalid")]
    InvalidSignature,
    #[error("human invite credential claims are unsupported")]
    UnsupportedClaims,
    #[error("human invite credential policy is invalid")]
    InvalidPolicy,
    #[error("human invite credential entropy source failed")]
    Entropy,
    #[error("human invite credential JSON encoding failed")]
    Json,
}

pub struct HumanInviteCredentialDraft {
    pub room_url: String,
    pub public_room_url: String,
    pub room_id: String,
    pub base_participant_id: String,
    pub display_name: String,
    pub invite_scope: InviteScope,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

pub struct IssuedHumanInviteCredentials {
    invite_token: String,
    join_code: String,
    signed_token_fingerprint: [u8; 32],
    join_code_fingerprint: [u8; 32],
}

impl IssuedHumanInviteCredentials {
    #[must_use]
    pub fn invite_token(&self) -> &str {
        &self.invite_token
    }

    #[must_use]
    pub fn join_code(&self) -> &str {
        &self.join_code
    }

    #[must_use]
    pub const fn signed_token_fingerprint(&self) -> &[u8; 32] {
        &self.signed_token_fingerprint
    }

    #[must_use]
    pub const fn join_code_fingerprint(&self) -> &[u8; 32] {
        &self.join_code_fingerprint
    }
}

#[derive(Clone)]
pub struct HumanInviteCredentialAuthority {
    key: Arc<[u8; 32]>,
}

impl HumanInviteCredentialAuthority {
    #[must_use]
    pub fn from_persistent(identity: &PersistentHostIdentity) -> Self {
        Self {
            key: Arc::new(*identity.session_hmac_key()),
        }
    }

    /// Creates the current signed invite token and independent join code.
    ///
    /// # Errors
    ///
    /// Rejects invalid policy, JSON encoding, or operating-system entropy failure.
    pub fn issue(
        &self,
        draft: &HumanInviteCredentialDraft,
    ) -> Result<IssuedHumanInviteCredentials, HumanInviteCredentialError> {
        let mut signed_nonce = [0_u8; SIGNED_NONCE_BYTES];
        let mut join_code = [0_u8; HUMAN_INVITE_JOIN_CODE_BYTES];
        let random = SystemRandom::new();
        random
            .fill(&mut signed_nonce)
            .map_err(|_| HumanInviteCredentialError::Entropy)?;
        random
            .fill(&mut join_code)
            .map_err(|_| HumanInviteCredentialError::Entropy)?;
        self.issue_with_material(draft, signed_nonce, join_code)
    }

    /// Authenticates one signed token or canonical join code without persistence access.
    ///
    /// Signed-token expiry is deliberately exposed in the authenticated claims instead
    /// of being enforced here. Admission must bind those claims to the durable invite
    /// and resolve an exact one-use retry before applying current-invite expiry.
    ///
    /// # Errors
    ///
    /// Rejects missing, malformed, invalidly signed, or unsupported credentials.
    pub fn authenticate(
        &self,
        credential: &str,
    ) -> Result<VerifiedHumanInviteCredential, HumanInviteCredentialError> {
        let credential = credential.trim();
        if credential.is_empty() {
            return Err(HumanInviteCredentialError::Missing);
        }
        if credential.starts_with(HUMAN_INVITE_JOIN_CODE_PREFIX) {
            return verify_join_code(credential);
        }
        self.authenticate_signed_token(credential)
    }

    fn issue_with_material(
        &self,
        draft: &HumanInviteCredentialDraft,
        signed_nonce: [u8; SIGNED_NONCE_BYTES],
        join_material: [u8; HUMAN_INVITE_JOIN_CODE_BYTES],
    ) -> Result<IssuedHumanInviteCredentials, HumanInviteCredentialError> {
        let claims = InviteClaims::from_draft(draft, signed_nonce)?;
        let payload = serde_json::to_vec(&claims).map_err(|_| HumanInviteCredentialError::Json)?;
        let encoded_payload = URL_SAFE_NO_PAD.encode(payload);
        let signing_input = format!("{HUMAN_INVITE_SIGNED_TOKEN_PREFIX}.{encoded_payload}");
        let signature = sign(&self.key, signing_input.as_bytes());
        let invite_token = format!("{signing_input}.{}", URL_SAFE_NO_PAD.encode(signature));
        let join_code = format!(
            "{HUMAN_INVITE_JOIN_CODE_PREFIX}{}",
            URL_SAFE_NO_PAD.encode(join_material)
        );
        Ok(IssuedHumanInviteCredentials {
            signed_token_fingerprint: fingerprint(invite_token.as_bytes()),
            join_code_fingerprint: fingerprint(join_code.as_bytes()),
            invite_token,
            join_code,
        })
    }

    fn authenticate_signed_token(
        &self,
        credential: &str,
    ) -> Result<VerifiedHumanInviteCredential, HumanInviteCredentialError> {
        if credential.len() > HUMAN_INVITE_SIGNED_TOKEN_MAX_BYTES || !credential.is_ascii() {
            return Err(HumanInviteCredentialError::Malformed);
        }
        let mut segments = credential.split('.');
        let prefix = segments.next().unwrap_or_default();
        let encoded_payload = segments.next().unwrap_or_default();
        let encoded_signature = segments.next().unwrap_or_default();
        if segments.next().is_some()
            || prefix != HUMAN_INVITE_SIGNED_TOKEN_PREFIX
            || encoded_payload.is_empty()
            || encoded_signature.is_empty()
            || !is_base64url(encoded_payload)
            || !is_base64url(encoded_signature)
        {
            return Err(HumanInviteCredentialError::Malformed);
        }
        let signature = decode_canonical(encoded_signature)?;
        let signature: [u8; HUMAN_INVITE_SIGNATURE_BYTES] = signature
            .try_into()
            .map_err(|_| HumanInviteCredentialError::Malformed)?;
        let signing_input = format!("{prefix}.{encoded_payload}");
        let expected = sign(&self.key, signing_input.as_bytes());
        if !bool::from(expected.ct_eq(&signature)) {
            return Err(HumanInviteCredentialError::InvalidSignature);
        }
        let payload = decode_canonical(encoded_payload)?;
        let claims: InviteClaims = serde_json::from_slice(&payload)
            .map_err(|_| HumanInviteCredentialError::UnsupportedClaims)?;
        let verified = claims.authenticate()?;
        Ok(VerifiedHumanInviteCredential::Signed {
            fingerprint: fingerprint(credential.as_bytes()),
            claims: verified,
        })
    }
}

pub enum VerifiedHumanInviteCredential {
    Signed {
        fingerprint: [u8; 32],
        claims: VerifiedHumanInviteClaims,
    },
    JoinCode {
        fingerprint: [u8; 32],
    },
}

impl VerifiedHumanInviteCredential {
    #[must_use]
    pub const fn fingerprint(&self) -> &[u8; 32] {
        match self {
            Self::Signed { fingerprint, .. } | Self::JoinCode { fingerprint } => fingerprint,
        }
    }

    #[must_use]
    pub const fn signed_claims(&self) -> Option<&VerifiedHumanInviteClaims> {
        match self {
            Self::Signed { claims, .. } => Some(claims),
            Self::JoinCode { .. } => None,
        }
    }
}

pub struct VerifiedHumanInviteClaims {
    pub room_url: String,
    pub public_room_url: String,
    pub room_id: String,
    pub base_participant_id: String,
    pub display_name: String,
    pub invite_scope: InviteScope,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

impl VerifiedHumanInviteClaims {
    #[must_use]
    pub fn is_expired_at(&self, now: DateTime<Utc>) -> bool {
        self.expires_at <= now
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct InviteClaims {
    admission: AdmissionClaims,
    agent: AgentClaims,
    client_kind: String,
    expires_at: String,
    issued_at: String,
    meeting_id: String,
    mode: String,
    nonce: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    public_room_url: Option<String>,
    room_host_scope: String,
    room_url: String,
    schema: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AdmissionClaims {
    host_verifies: [String; 4],
    identity_proof: String,
    permission_mode: String,
    provider_execution: String,
    remote_http_bridge: bool,
    remote_transport: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AgentClaims {
    agent_id: String,
    display_name: String,
    provider_kind: String,
}

impl InviteClaims {
    fn from_draft(
        draft: &HumanInviteCredentialDraft,
        nonce: [u8; SIGNED_NONCE_BYTES],
    ) -> Result<Self, HumanInviteCredentialError> {
        if !canonical_text(&draft.room_id, 128)
            || !canonical_text(&draft.base_participant_id, 64)
            || !canonical_text(&draft.display_name, 128)
            || !exact_microsecond(draft.issued_at)
            || !exact_microsecond(draft.expires_at)
            || draft.expires_at <= draft.issued_at
            || (!draft.public_room_url.is_empty()
                && !canonical_text(&draft.public_room_url, MAX_PUBLIC_ROOM_URL_CHARS))
        {
            return Err(HumanInviteCredentialError::InvalidPolicy);
        }
        let (room_url, room_host_scope) = normalize_room_url(&draft.room_url)?;
        Ok(Self {
            admission: admission_claims(draft.invite_scope),
            agent: AgentClaims {
                agent_id: draft.base_participant_id.clone(),
                display_name: draft.display_name.clone(),
                provider_kind: HUMAN_PROVIDER_KIND.to_owned(),
            },
            client_kind: CLAIM_CLIENT_KIND.to_owned(),
            expires_at: format_invite_timestamp(draft.expires_at),
            issued_at: format_invite_timestamp(draft.issued_at),
            meeting_id: draft.room_id.clone(),
            mode: CLAIM_MODE.to_owned(),
            nonce: URL_SAFE_NO_PAD.encode(nonce),
            public_room_url: (!draft.public_room_url.is_empty())
                .then(|| draft.public_room_url.clone()),
            room_host_scope,
            room_url,
            schema: CLAIM_SCHEMA.to_owned(),
        })
    }

    fn authenticate(self) -> Result<VerifiedHumanInviteClaims, HumanInviteCredentialError> {
        if self.schema != CLAIM_SCHEMA
            || self.mode != CLAIM_MODE
            || self.client_kind != CLAIM_CLIENT_KIND
            || self.agent.provider_kind != HUMAN_PROVIDER_KIND
            || !canonical_text(&self.meeting_id, 128)
            || !canonical_text(&self.agent.agent_id, 64)
            || !canonical_text(&self.agent.display_name, 128)
            || !is_canonical_nonce(&self.nonce)
            || !admission_is_current(&self.admission)
            || self
                .public_room_url
                .as_ref()
                .is_some_and(|value| !canonical_text(value, MAX_PUBLIC_ROOM_URL_CHARS))
        {
            return Err(HumanInviteCredentialError::UnsupportedClaims);
        }
        let (room_url, room_host_scope) = normalize_room_url(&self.room_url)?;
        if room_url != self.room_url || room_host_scope != self.room_host_scope {
            return Err(HumanInviteCredentialError::UnsupportedClaims);
        }
        let issued_at = parse_canonical_timestamp(&self.issued_at)?;
        let expires_at = parse_canonical_timestamp(&self.expires_at)?;
        if expires_at <= issued_at {
            return Err(HumanInviteCredentialError::UnsupportedClaims);
        }
        let invite_scope = match self.admission.permission_mode.as_str() {
            "participant" => InviteScope::ReadWrite,
            "meeting_read_only" => InviteScope::ReadOnly,
            _ => return Err(HumanInviteCredentialError::UnsupportedClaims),
        };
        Ok(VerifiedHumanInviteClaims {
            room_url,
            public_room_url: self.public_room_url.unwrap_or_default(),
            room_id: self.meeting_id,
            base_participant_id: self.agent.agent_id,
            display_name: self.agent.display_name,
            invite_scope,
            issued_at,
            expires_at,
        })
    }
}

fn admission_claims(invite_scope: InviteScope) -> AdmissionClaims {
    AdmissionClaims {
        host_verifies: [
            "token_signature".to_owned(),
            "token_expiry".to_owned(),
            "meeting_id".to_owned(),
            "agent_id".to_owned(),
        ],
        identity_proof: "hmac_sha256_invite_token".to_owned(),
        permission_mode: match invite_scope {
            InviteScope::ReadWrite => "participant",
            InviteScope::ReadOnly => "meeting_read_only",
        }
        .to_owned(),
        provider_execution: "not_started_by_invite".to_owned(),
        remote_http_bridge: false,
        remote_transport: CLAIM_CLIENT_KIND.to_owned(),
    }
}

fn admission_is_current(claims: &AdmissionClaims) -> bool {
    claims.host_verifies == ["token_signature", "token_expiry", "meeting_id", "agent_id"]
        && claims.identity_proof == "hmac_sha256_invite_token"
        && matches!(
            claims.permission_mode.as_str(),
            "participant" | "meeting_read_only"
        )
        && claims.provider_execution == "not_started_by_invite"
        && !claims.remote_http_bridge
        && claims.remote_transport == CLAIM_CLIENT_KIND
}

fn verify_join_code(
    credential: &str,
) -> Result<VerifiedHumanInviteCredential, HumanInviteCredentialError> {
    if credential.len()
        != HUMAN_INVITE_JOIN_CODE_PREFIX.len() + HUMAN_INVITE_JOIN_CODE_BYTES * 4 / 3
    {
        return Err(HumanInviteCredentialError::Malformed);
    }
    let encoded = &credential[HUMAN_INVITE_JOIN_CODE_PREFIX.len()..];
    let material = decode_canonical(encoded)?;
    if material.len() != HUMAN_INVITE_JOIN_CODE_BYTES {
        return Err(HumanInviteCredentialError::Malformed);
    }
    Ok(VerifiedHumanInviteCredential::JoinCode {
        fingerprint: fingerprint(credential.as_bytes()),
    })
}

fn normalize_room_url(value: &str) -> Result<(String, String), HumanInviteCredentialError> {
    let mut url = Url::parse(value).map_err(|_| HumanInviteCredentialError::InvalidPolicy)?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(HumanInviteCredentialError::InvalidPolicy);
    }
    let host = url
        .host()
        .ok_or(HumanInviteCredentialError::InvalidPolicy)?;
    let host_scope = match host {
        Host::Domain(domain) if domain.eq_ignore_ascii_case("localhost") => "loopback",
        Host::Domain(domain) if is_local_hostname(domain) => "lan_hostname",
        Host::Domain(domain) if !domain.contains('.') => "host_or_lan_ip",
        Host::Ipv4(address)
            if address.is_loopback() || address.is_private() || address.is_link_local() =>
        {
            if address.is_loopback() {
                "loopback"
            } else {
                "host_or_lan_ip"
            }
        }
        Host::Ipv6(address)
            if address.is_loopback()
                || address.is_unique_local()
                || address.is_unicast_link_local() =>
        {
            if address.is_loopback() {
                "loopback"
            } else {
                "host_or_lan_ip"
            }
        }
        Host::Domain(_) | Host::Ipv4(_) | Host::Ipv6(_) => {
            return Err(HumanInviteCredentialError::InvalidPolicy);
        }
    };
    if url.scheme() == "http" && !url.host_str().is_some_and(is_loopback_http_host) {
        return Err(HumanInviteCredentialError::InvalidPolicy);
    }
    if url.path() == "/" {
        url.set_path("");
    } else if url.path().ends_with('/') {
        let path = url.path().trim_end_matches('/').to_owned();
        url.set_path(&path);
    }
    Ok((
        url.to_string().trim_end_matches('/').to_owned(),
        host_scope.to_owned(),
    ))
}

fn is_local_hostname(domain: &str) -> bool {
    domain.contains('.')
        && domain
            .rsplit('.')
            .next()
            .is_some_and(|suffix| suffix.eq_ignore_ascii_case("local"))
}

fn parse_canonical_timestamp(value: &str) -> Result<DateTime<Utc>, HumanInviteCredentialError> {
    let timestamp = DateTime::parse_from_rfc3339(value)
        .map_err(|_| HumanInviteCredentialError::UnsupportedClaims)?
        .with_timezone(&Utc);
    if !exact_microsecond(timestamp) || format_invite_timestamp(timestamp) != value {
        return Err(HumanInviteCredentialError::UnsupportedClaims);
    }
    Ok(timestamp)
}

pub(crate) fn format_invite_timestamp(value: DateTime<Utc>) -> String {
    let precision = if value.timestamp_subsec_nanos() == 0 {
        SecondsFormat::Secs
    } else {
        SecondsFormat::Micros
    };
    value.to_rfc3339_opts(precision, false)
}

fn exact_microsecond(value: DateTime<Utc>) -> bool {
    value.timestamp_subsec_nanos().is_multiple_of(1_000)
}

fn canonical_text(value: &str, max_chars: usize) -> bool {
    !value.is_empty()
        && value.trim() == value
        && value.chars().count() <= max_chars
        && !value.chars().any(char::is_control)
}

fn is_canonical_nonce(value: &str) -> bool {
    decode_canonical(value).is_ok_and(|bytes| bytes.len() == SIGNED_NONCE_BYTES)
}

fn is_base64url(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn decode_canonical(value: &str) -> Result<Vec<u8>, HumanInviteCredentialError> {
    if !is_base64url(value) {
        return Err(HumanInviteCredentialError::Malformed);
    }
    let decoded = URL_SAFE_NO_PAD
        .decode(value)
        .map_err(|_| HumanInviteCredentialError::Malformed)?;
    if URL_SAFE_NO_PAD.encode(&decoded) != value {
        return Err(HumanInviteCredentialError::Malformed);
    }
    Ok(decoded)
}

fn sign(key: &[u8; 32], input: &[u8]) -> [u8; 32] {
    let mut signer = Hmac::<Sha256>::new_from_slice(key)
        .unwrap_or_else(|_| unreachable!("HMAC accepts a 32-byte key"));
    signer.update(input);
    signer.finalize().into_bytes().into()
}

fn fingerprint(value: &[u8]) -> [u8; 32] {
    Sha256::digest(value).into()
}

#[cfg(test)]
mod tests {
    use chrono::{Datelike, NaiveDate, TimeZone, Utc};

    use super::{
        HumanInviteCredentialAuthority, HumanInviteCredentialDraft, HumanInviteCredentialError,
        VerifiedHumanInviteCredential,
    };
    use agentsassemble_domain::InviteScope;
    use agentsassemble_protocol::{
        HUMAN_INVITE_TIMESTAMP_MAX_YEAR, HUMAN_INVITE_TIMESTAMP_MIN_YEAR,
    };

    fn authority() -> HumanInviteCredentialAuthority {
        HumanInviteCredentialAuthority {
            key: std::sync::Arc::new([0x11; 32]),
        }
    }

    fn draft() -> HumanInviteCredentialDraft {
        HumanInviteCredentialDraft {
            room_url: "https://192.168.1.50:8765".to_owned(),
            public_room_url: "https://room.example.test".to_owned(),
            room_id: "general".to_owned(),
            base_participant_id: "guest-abcd1234".to_owned(),
            display_name: "Guest Human".to_owned(),
            invite_scope: InviteScope::ReadOnly,
            issued_at: Utc
                .with_ymd_and_hms(2026, 8, 26, 1, 2, 3)
                .single()
                .unwrap_or_else(|| panic!("fixed issued_at")),
            expires_at: Utc
                .with_ymd_and_hms(2026, 8, 26, 2, 2, 3)
                .single()
                .unwrap_or_else(|| panic!("fixed expires_at")),
        }
    }

    #[test]
    fn timestamp_wire_years_match_the_pinned_chrono_domain() {
        assert_eq!(NaiveDate::MIN.year(), HUMAN_INVITE_TIMESTAMP_MIN_YEAR);
        assert_eq!(NaiveDate::MAX.year(), HUMAN_INVITE_TIMESTAMP_MAX_YEAR);
    }

    #[test]
    fn fixed_vectors_preserve_both_current_credentials_and_signed_claims() {
        let issued = authority()
            .issue_with_material(&draft(), [0x22; 18], [0x33; 24])
            .unwrap_or_else(|error| panic!("issue fixed credentials: {error}"));
        assert_eq!(issued.join_code(), "aaj1_MzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMz");
        assert_eq!(
            issued.invite_token(),
            concat!(
                "aai1.eyJhZG1pc3Npb24iOnsiaG9zdF92ZXJpZmllcyI6WyJ0b2tlbl9zaWduYXR1cmUiLC",
                "J0b2tlbl9leHBpcnkiLCJtZWV0aW5nX2lkIiwiYWdlbnRfaWQiXSwiaWRlbnRpdHlf",
                "cHJvb2YiOiJobWFjX3NoYTI1Nl9pbnZpdGVfdG9rZW4iLCJwZXJtaXNzaW9uX21vZG",
                "UiOiJtZWV0aW5nX3JlYWRfb25seSIsInByb3ZpZGVyX2V4ZWN1dGlvbiI6Im5vdF9z",
                "dGFydGVkX2J5X2ludml0ZSIsInJlbW90ZV9odHRwX2JyaWRnZSI6ZmFsc2UsInJlbW",
                "90ZV90cmFuc3BvcnQiOiJuYXRpdmVfcmVtb3RlX3Jvb21fY2xpZW50In0sImFnZW50",
                "Ijp7ImFnZW50X2lkIjoiZ3Vlc3QtYWJjZDEyMzQiLCJkaXNwbGF5X25hbWUiOiJHdWV",
                "zdCBIdW1hbiIsInByb3ZpZGVyX2tpbmQiOiJtYW51YWwifSwiY2xpZW50X2tpbmQiOi",
                "JuYXRpdmVfcmVtb3RlX3Jvb21fY2xpZW50IiwiZXhwaXJlc19hdCI6IjIwMjYtMDgtMj",
                "ZUMDI6MDI6MDMrMDA6MDAiLCJpc3N1ZWRfYXQiOiIyMDI2LTA4LTI2VDAxOjAyOjAz",
                "KzAwOjAwIiwibWVldGluZ19pZCI6ImdlbmVyYWwiLCJtb2RlIjoibGFuX2ludml0ZV9",
                "0b2tlbiIsIm5vbmNlIjoiSWlJaUlpSWlJaUlpSWlJaUlpSWlJaUlpIiwicHVibGljX3",
                "Jvb21fdXJsIjoiaHR0cHM6Ly9yb29tLmV4YW1wbGUudGVzdCIsInJvb21faG9zdF9zY",
                "29wZSI6Imhvc3Rfb3JfbGFuX2lwIiwicm9vbV91cmwiOiJodHRwczovLzE5Mi4xNjgu",
                "MS41MDo4NzY1Iiwic2NoZW1hIjoiYWdlbnRzYXNzZW1ibGUubGFuX2ludml0ZS52MS",
                "J9.ttXkmpTI7aLBEesDWlP4Vm3Ce2JLNiZn1PBW0z2JZL8"
            )
        );
        assert_eq!(
            authority()
                .authenticate(issued.invite_token())
                .unwrap_or_else(|error| panic!("authenticate signed credential: {error}"))
                .fingerprint(),
            issued.signed_token_fingerprint()
        );
        assert_eq!(
            authority()
                .authenticate(issued.join_code())
                .unwrap_or_else(|error| panic!("authenticate join credential: {error}"))
                .fingerprint(),
            issued.join_code_fingerprint()
        );
    }

    #[test]
    fn authentication_separates_expiry_from_credential_integrity() {
        let issued = authority()
            .issue_with_material(&draft(), [0x22; 18], [0x33; 24])
            .unwrap_or_else(|error| panic!("issue fixed credentials: {error}"));
        let mut tampered = issued.invite_token().as_bytes().to_vec();
        let last = tampered.len() - 1;
        tampered[last] = if tampered[last] == b'A' { b'B' } else { b'A' };
        let tampered = String::from_utf8(tampered)
            .unwrap_or_else(|error| panic!("tampered credential remains UTF-8: {error}"));
        assert!(matches!(
            authority().authenticate(&tampered),
            Err(HumanInviteCredentialError::InvalidSignature)
        ));
        let authenticated = authority()
            .authenticate(issued.invite_token())
            .unwrap_or_else(|error| panic!("authenticate expired signed credential: {error}"));
        let claims = authenticated
            .signed_claims()
            .unwrap_or_else(|| panic!("signed credential claims"));
        assert!(!claims.is_expired_at(draft().expires_at - chrono::Duration::microseconds(1)));
        assert!(claims.is_expired_at(draft().expires_at));
        assert!(matches!(
            authority().authenticate("aaj1_AA=="),
            Err(HumanInviteCredentialError::Malformed)
        ));
        let oversized = "aai1.a.".to_owned() + &"a".repeat(5_000);
        assert!(matches!(
            authority().authenticate(&oversized),
            Err(HumanInviteCredentialError::Malformed)
        ));
        assert!(matches!(
            authority()
                .authenticate(issued.join_code())
                .unwrap_or_else(|error| panic!("authenticate join code: {error}")),
            VerifiedHumanInviteCredential::JoinCode { .. }
        ));
    }

    #[test]
    fn issuance_rejects_lossy_time_and_unsafe_room_transport() {
        let mut invalid_time = draft();
        invalid_time.expires_at += chrono::Duration::nanoseconds(1);
        assert!(matches!(
            authority().issue(&invalid_time),
            Err(HumanInviteCredentialError::InvalidPolicy)
        ));
        let mut public_host = draft();
        public_host.room_url = "https://example.com".to_owned();
        assert!(matches!(
            authority().issue(&public_host),
            Err(HumanInviteCredentialError::InvalidPolicy)
        ));
        let mut insecure_lan = draft();
        insecure_lan.room_url = "http://192.168.1.50:8765".to_owned();
        assert!(matches!(
            authority().issue(&insecure_lan),
            Err(HumanInviteCredentialError::InvalidPolicy)
        ));
    }
}
