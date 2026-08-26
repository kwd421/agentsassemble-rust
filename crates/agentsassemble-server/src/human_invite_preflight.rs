use agentsassemble_persistence::{
    HumanInviteCredentialEvidence, HumanInvitePreflight, HumanInvitePreflightRequest,
    PersistenceError, SqliteStore,
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::{
    HumanInviteCredentialAuthority, HumanInviteCredentialError, VerifiedHumanInviteCredential,
};

const BROWSER_CREDENTIAL_PREFIX: &str = "aad1_";
const SESSION_BEARER_PREFIX: &str = "aas1.";
const CLIENT_CREDENTIAL_BYTES: usize = 32;
const CLIENT_CREDENTIAL_BODY_CHARS: usize = 43;
const CLIENT_CREDENTIAL_CHARS: usize = 48;

#[derive(Debug, Error)]
pub enum HumanInvitePreflightError {
    #[error("human invite credential authentication failed")]
    InviteCredential(#[from] HumanInviteCredentialError),
    #[error("browser credential is malformed")]
    BrowserCredential,
    #[error("room session bearer is malformed")]
    SessionBearer,
    #[error("human invite preflight persistence failed")]
    Persistence(#[from] PersistenceError),
}

/// Authenticates every raw browser credential before entering persistence.
///
/// Raw values remain server-local and only complete SHA-256 fingerprints cross the
/// durable boundary. The caller must not log this function's string arguments.
///
/// # Errors
///
/// Returns a typed error when the invite cannot be authenticated, either client
/// credential is noncanonical, or the durable snapshot cannot be read.
pub async fn preflight_human_invite(
    store: &SqliteStore,
    authority: &HumanInviteCredentialAuthority,
    invite_credential: &str,
    browser_credential: &str,
    session_bearer: Option<&str>,
    now: DateTime<Utc>,
) -> Result<HumanInvitePreflight, HumanInvitePreflightError> {
    let credential = authenticated_invite_evidence(authority, invite_credential)?;
    let browser_credential_fingerprint =
        fingerprint_client_credential(browser_credential, BROWSER_CREDENTIAL_PREFIX)
            .ok_or(HumanInvitePreflightError::BrowserCredential)?;
    let session_fingerprint = session_bearer
        .map(|bearer| {
            fingerprint_client_credential(bearer, SESSION_BEARER_PREFIX)
                .ok_or(HumanInvitePreflightError::SessionBearer)
        })
        .transpose()?;
    Ok(store
        .preflight_human_invite(&HumanInvitePreflightRequest {
            credential,
            session_fingerprint,
            browser_credential_fingerprint: Some(browser_credential_fingerprint),
            now,
        })
        .await?)
}

fn authenticated_invite_evidence(
    authority: &HumanInviteCredentialAuthority,
    credential: &str,
) -> Result<HumanInviteCredentialEvidence, HumanInviteCredentialError> {
    Ok(match authority.authenticate(credential)? {
        VerifiedHumanInviteCredential::Signed {
            fingerprint,
            claims,
        } => HumanInviteCredentialEvidence::Signed {
            fingerprint,
            room_id: claims.room_id,
            base_participant_id: claims.base_participant_id,
            display_name: claims.display_name,
            invite_scope: claims.invite_scope,
            issued_at: claims.issued_at,
            expires_at: claims.expires_at,
        },
        VerifiedHumanInviteCredential::JoinCode { fingerprint } => {
            HumanInviteCredentialEvidence::JoinCode { fingerprint }
        }
    })
}

fn fingerprint_client_credential(value: &str, prefix: &str) -> Option<[u8; 32]> {
    if value.len() != CLIENT_CREDENTIAL_CHARS || !value.is_ascii() {
        return None;
    }
    let encoded = value.strip_prefix(prefix)?;
    if encoded.len() != CLIENT_CREDENTIAL_BODY_CHARS
        || !encoded
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return None;
    }
    let decoded = URL_SAFE_NO_PAD.decode(encoded).ok()?;
    if decoded.len() != CLIENT_CREDENTIAL_BYTES || URL_SAFE_NO_PAD.encode(decoded) != encoded {
        return None;
    }
    Some(Sha256::digest(value.as_bytes()).into())
}

#[cfg(test)]
mod tests {
    use agentsassemble_domain::{
        InviteScope, LOCAL_OPERATOR_PARTICIPANT_ID, LOCAL_OPERATOR_USER_ID,
    };
    use agentsassemble_persistence::{HumanInvitePreflight, NewHumanInvite, SqliteStore};
    use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
    use chrono::{TimeZone, Utc};
    use sha2::{Digest, Sha256};

    use super::{
        BROWSER_CREDENTIAL_PREFIX, HumanInvitePreflightError, SESSION_BEARER_PREFIX,
        fingerprint_client_credential, preflight_human_invite,
    };
    use crate::{HumanInviteCredentialAuthority, HumanInviteCredentialDraft};

    #[test]
    fn client_credentials_are_canonical_fixed_size_domains() {
        let body = URL_SAFE_NO_PAD.encode([0xA5; 32]);
        let browser = format!("{BROWSER_CREDENTIAL_PREFIX}{body}");
        let session = format!("{SESSION_BEARER_PREFIX}{body}");
        assert_eq!(
            fingerprint_client_credential(&browser, BROWSER_CREDENTIAL_PREFIX),
            Some(Sha256::digest(browser.as_bytes()).into())
        );
        assert_eq!(
            fingerprint_client_credential(&session, SESSION_BEARER_PREFIX),
            Some(Sha256::digest(session.as_bytes()).into())
        );
        assert_ne!(
            fingerprint_client_credential(&browser, BROWSER_CREDENTIAL_PREFIX),
            fingerprint_client_credential(&session, SESSION_BEARER_PREFIX)
        );

        for malformed in [
            String::new(),
            format!("{BROWSER_CREDENTIAL_PREFIX}{}", &body[..42]),
            format!("{BROWSER_CREDENTIAL_PREFIX}{body}="),
            format!(" {browser}"),
            format!("{SESSION_BEARER_PREFIX}{body}"),
        ] {
            assert_eq!(
                fingerprint_client_credential(&malformed, BROWSER_CREDENTIAL_PREFIX),
                None
            );
        }
    }

    #[tokio::test]
    async fn raw_preflight_authenticates_and_binds_current_invite() {
        let store = fixture().await;
        let identity = store
            .host_identity()
            .await
            .unwrap_or_else(|error| panic!("load host identity: {error}"));
        let authority = HumanInviteCredentialAuthority::from_persistent(&identity);
        let issued_at = micros(1_000_000);
        let expires_at = micros(5_000_000);
        let draft = HumanInviteCredentialDraft {
            room_url: "http://127.0.0.1:8765".to_owned(),
            public_room_url: String::new(),
            room_id: "general".to_owned(),
            base_participant_id: "invite-guest".to_owned(),
            display_name: "Invite Guest".to_owned(),
            invite_scope: InviteScope::ReadWrite,
            issued_at,
            expires_at,
        };
        let issued = authority
            .issue(&draft)
            .unwrap_or_else(|error| panic!("issue human invite credentials: {error}"));
        let manager = store
            .authorize_local_room_manager(
                "general",
                LOCAL_OPERATOR_USER_ID,
                LOCAL_OPERATOR_PARTICIPANT_ID,
            )
            .await
            .unwrap_or_else(|error| panic!("authorize room manager: {error}"));
        store
            .create_human_invite_for_local_manager(
                &manager,
                NewHumanInvite {
                    signed_token_fingerprint: *issued.signed_token_fingerprint(),
                    join_code_fingerprint: *issued.join_code_fingerprint(),
                    base_participant_id: draft.base_participant_id,
                    display_name: draft.display_name,
                    invite_scope: draft.invite_scope,
                    max_uses: 5,
                    expires_at,
                    created_at: issued_at,
                },
            )
            .await
            .unwrap_or_else(|error| panic!("persist human invite: {error}"));

        let browser = format!(
            "{BROWSER_CREDENTIAL_PREFIX}{}",
            URL_SAFE_NO_PAD.encode([0xB6; 32])
        );
        for credential in [issued.invite_token(), issued.join_code()] {
            let decision = preflight_human_invite(
                &store,
                &authority,
                credential,
                &browser,
                None,
                micros(2_000_000),
            )
            .await
            .unwrap_or_else(|error| panic!("preflight current credential: {error}"));
            assert!(matches!(decision, HumanInvitePreflight::ProfileRequired(_)));
        }
        assert!(matches!(
            preflight_human_invite(
                &store,
                &authority,
                issued.invite_token(),
                "legacy-device-token",
                None,
                micros(2_000_000),
            )
            .await,
            Err(HumanInvitePreflightError::BrowserCredential)
        ));
        assert!(matches!(
            preflight_human_invite(
                &store,
                &authority,
                issued.invite_token(),
                &browser,
                Some("legacy-session-token"),
                micros(2_000_000),
            )
            .await,
            Err(HumanInvitePreflightError::SessionBearer)
        ));
    }

    async fn fixture() -> SqliteStore {
        let store = SqliteStore::open("sqlite::memory:")
            .await
            .unwrap_or_else(|error| panic!("open store: {error}"));
        store
            .bootstrap_local_authority("e5f63872-a170-4e34-98af-55940ff4a91a", "Host")
            .await
            .unwrap_or_else(|error| panic!("bootstrap authority: {error}"));
        store
            .create_room_for_local_operator(
                "15ebaf41-12b9-4b30-94d1-d62435b30fba",
                "general",
                "General",
            )
            .await
            .unwrap_or_else(|error| panic!("create room: {error}"));
        store
    }

    fn micros(value: i64) -> chrono::DateTime<Utc> {
        Utc.timestamp_micros(value)
            .single()
            .unwrap_or_else(|| panic!("valid timestamp"))
    }
}
