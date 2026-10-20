//! Host-owned member resume keys.
//!
//! A resume key is a public P-256 key a member device generates itself and
//! the host stores after a central admission. The private half never reaches
//! this host. A direct reconnect proves possession of that key over a fresh
//! secure channel, so the member reaches its joined rooms without spending a
//! central grant.
use super::*;
use agentsassemble_persistence::SecureSessionBinding;
use ring::rand::SecureRandom as _;
use ring::signature::{ECDSA_P256_SHA256_FIXED, UnparsedPublicKey};
use std::collections::HashMap;
use std::sync::Arc;

/// The domain separator of the resume transcript. The client builds the
/// signed bytes itself; this host never sends a blob to sign.
pub(crate) const RESUME_PROTOCOL: &str = "AA-MEMBER-RESUME-1";
/// The purpose both sides sign.
pub(crate) const RESUME_PURPOSE: &str = "resume";
/// How long a host-issued resume nonce stays usable.
const RESUME_NONCE_TTL: chrono::Duration = chrono::Duration::seconds(300);

/// The exact bytes both sides sign over a resume attempt.
///
/// `client_public_key` is the ephemeral key of the channel the request
/// arrived on, so the signature is bound to that channel; the device's own
/// resume public key travels beside it and selects the stored row.
pub(crate) fn resume_transcript(
    server_id: &str,
    registration_epoch: &str,
    origin: &str,
    generation: i64,
    host_key_fingerprint: &str,
    browser_credential_fingerprint: &str,
    client_public_key: &str,
    channel_id: &str,
    nonce: &str,
) -> Vec<u8> {
    json!([
        RESUME_PROTOCOL,
        server_id,
        registration_epoch,
        origin,
        generation,
        host_key_fingerprint,
        browser_credential_fingerprint,
        client_public_key,
        channel_id,
        RESUME_PURPOSE,
        nonce
    ])
    .to_string()
    .into_bytes()
}

/// Host-issued single-use nonces, each bound to one secure channel.
#[derive(Default)]
pub(crate) struct MemberResumeNonces(parking_lot::Mutex<HashMap<[u8; 32], ResumeNonce>>);

struct ResumeNonce {
    channel_id: String,
    client_key_fingerprint: [u8; 32],
    registration_epoch: String,
    expires_at: chrono::DateTime<chrono::Utc>,
}

impl MemberResumeNonces {
    /// Issues one nonce for this exact channel binding.
    fn issue(
        &self,
        binding: &SecureSessionBinding,
        registration_epoch: &str,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<[u8; 32], HumanInviteHttpError> {
        let mut nonce = [0u8; 32];
        ring::rand::SystemRandom::new()
            .fill(&mut nonce)
            .map_err(|_| invalid_resume())?;
        let mut entries = self.0.lock();
        entries.retain(|_, entry| entry.expires_at > now);
        entries.insert(
            nonce,
            ResumeNonce {
                channel_id: binding.channel_id.clone(),
                client_key_fingerprint: binding.client_key_fingerprint,
                registration_epoch: registration_epoch.to_owned(),
                expires_at: now + RESUME_NONCE_TTL,
            },
        );
        Ok(nonce)
    }

    /// Consumes the nonce exactly once. A nonce issued for another channel,
    /// another incarnation or an expired one never opens the direct path.
    fn consume(
        &self,
        nonce: &[u8; 32],
        binding: &SecureSessionBinding,
        registration_epoch: &str,
        now: chrono::DateTime<chrono::Utc>,
    ) -> bool {
        let mut entries = self.0.lock();
        let Some(entry) = entries.remove(nonce) else {
            return false;
        };
        entry.expires_at > now
            && entry.channel_id == binding.channel_id
            && entry.client_key_fingerprint == binding.client_key_fingerprint
            && entry.registration_epoch == registration_epoch
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IssueKeyRequest {
    client_public_key: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResumeRequest {
    server_id: String,
    registration_epoch: String,
    origin: String,
    generation: i64,
    host_key_fingerprint: String,
    browser_credential_fingerprint: String,
    client_public_key: String,
    resume_public_key: String,
    channel_id: String,
    purpose: String,
    nonce: String,
    signature: String,
}

fn invalid_resume() -> HumanInviteHttpError {
    HumanInviteHttpError::unauthorized(
        "member_resume_invalid",
        "This device cannot resume the member session.",
    )
}

fn decode_fixed(encoded: &str, length: usize) -> Result<Vec<u8>, HumanInviteHttpError> {
    let bytes = URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|_| invalid_resume())?;
    if bytes.len() != length || URL_SAFE_NO_PAD.encode(&bytes) != encoded {
        return Err(invalid_resume());
    }
    Ok(bytes)
}

fn decode_public_key(encoded: &str) -> Result<[u8; 65], HumanInviteHttpError> {
    let bytes = decode_fixed(encoded, 65)?;
    let mut key = [0u8; 65];
    key.copy_from_slice(&bytes);
    if key[0] != 4 {
        return Err(invalid_resume());
    }
    Ok(key)
}

/// Stores the resume key of a member device after a central admission.
///
/// The person, issuer and incarnation come from the redeemed admission on
/// this channel; the client only contributes its public key. The expiry is
/// the host's own room session lifetime. Only a central grant redeems a
/// challenge that may issue a key, and it issues it exactly once: a direct
/// resume never issues or refreshes a key.
pub(crate) async fn issue_key(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<Value>, HumanInviteHttpError> {
    let secure_client = crate::secure_client::from_request(&request).ok_or_else(invalid_resume)?;
    let browser = browser(request.headers())?;
    let body: IssueKeyRequest = decode_json_body(request, MAX_ADMISSION_BODY_BYTES)
        .await
        .map_err(HumanInviteHttpError::from_body)?;
    let public_key = decode_public_key(&body.client_public_key)?;
    let member = state
        .member_challenges
        .claim_connect_resume_key(secure_client.binding())
        .await
        .ok_or_else(invalid_resume)?;
    let epoch = state
        .store
        .registration_epoch()
        .await?
        .ok_or_else(invalid_resume)?;
    if member.registration_epoch != epoch {
        return Err(invalid_resume());
    }
    // The key belongs to the membership projection this host already stores:
    // a projection that differs from the stored one is never used.
    let stored = state
        .store
        .member_projection_anchor(&member.issuer, &member.person_id, &epoch)
        .await?;
    if stored.as_deref() != Some(member.projection_id.as_str()) {
        return Err(invalid_resume());
    }
    let now = Utc::now();
    let expires = now + state.store.human_room_session_ttl();
    state
        .store
        .issue_member_resume_key(agentsassemble_persistence::MemberResumeKeyIssue {
            issuer: &member.issuer,
            person_id: &member.person_id,
            registration_epoch: &epoch,
            browser_credential_fingerprint: &browser,
            resume_public_key: &public_key,
            issued_at: now,
            expires_at: expires,
        })
        .await?;
    Ok(Json(
        json!({"issued_at": now.timestamp(), "expires_at": expires.timestamp()}),
    ))
}

/// Issues one single-use nonce for this channel binding.
pub(crate) async fn issue_nonce(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<Value>, HumanInviteHttpError> {
    let secure_client = crate::secure_client::from_request(&request).ok_or_else(invalid_resume)?;
    let epoch = state
        .store
        .registration_epoch()
        .await?
        .ok_or_else(invalid_resume)?;
    let now = Utc::now();
    let nonce = state
        .member_resume_nonces
        .issue(secure_client.binding(), &epoch, now)?;
    Ok(Json(
        json!({"nonce": URL_SAFE_NO_PAD.encode(nonce), "expires_at": (now + RESUME_NONCE_TTL).timestamp()}),
    ))
}

/// Re-proves a member device with its resume key and lists its joined rooms.
///
/// The signature covers the channel this request arrived on, so a captured
/// signature never works on another channel, origin or incarnation. The
/// membership owners stay the unchanged ones: `member_connect_rooms` and
/// `select_member_connect_room`.
pub(crate) async fn resume(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<Value>, HumanInviteHttpError> {
    let secure_client = crate::secure_client::from_request(&request).ok_or_else(invalid_resume)?;
    let browser = browser(request.headers())?;
    let body: ResumeRequest = decode_json_body(request, MAX_ADMISSION_BODY_BYTES)
        .await
        .map_err(HumanInviteHttpError::from_body)?;
    if body.purpose != RESUME_PURPOSE {
        return Err(invalid_resume());
    }
    let hello = secure_client.hello();
    let binding = secure_client.binding();
    // The transcript names this channel's own binding, so a signature is
    // useless on any other channel, origin or incarnation.
    if body.server_id != hello.server_id
        || body.registration_epoch != hello.registration_epoch
        || body.origin != hello.origin
        || body.generation != hello.generation
        || body.client_public_key != hello.client_public_key
        || body.channel_id != binding.channel_id
        || body.host_key_fingerprint != state.central_host_identity.fingerprint()
    {
        return Err(invalid_resume());
    }
    if URL_SAFE_NO_PAD
        .decode(body.browser_credential_fingerprint.as_bytes())
        .map_err(|_| invalid_resume())?
        != browser.to_vec()
    {
        return Err(invalid_resume());
    }
    let public_key = decode_public_key(&body.resume_public_key)?;
    let nonce: [u8; 32] = decode_fixed(&body.nonce, 32)?.try_into().map_err(|_| invalid_resume())?;
    let signature = decode_fixed(&body.signature, 64)?;
    let epoch = state
        .store
        .registration_epoch()
        .await?
        .ok_or_else(invalid_resume)?;
    if body.registration_epoch != epoch {
        return Err(invalid_resume());
    }
    // The stored key is the only accepted public key: a direct attempt never
    // enrolls a new one, and an expired or foreign row is not a key.
    let key = state
        .store
        .member_resume_key(&browser, &public_key)
        .await?
        .ok_or_else(invalid_resume)?;
    let now = Utc::now();
    if key.expires_at <= now {
        return Err(invalid_resume());
    }
    let transcript = resume_transcript(
        &body.server_id,
        &body.registration_epoch,
        &body.origin,
        body.generation,
        &body.host_key_fingerprint,
        &body.browser_credential_fingerprint,
        &body.client_public_key,
        &body.channel_id,
        &body.nonce,
    );
    UnparsedPublicKey::new(&ECDSA_P256_SHA256_FIXED, key.resume_public_key.as_slice())
        .verify(&transcript, &signature)
        .map_err(|_| invalid_resume())?;
    // The nonce is single use: a replay on this channel finds nothing.
    if !state
        .member_resume_nonces
        .consume(&nonce, binding, &epoch, now)
    {
        return Err(invalid_resume());
    }
    let mut member = state
        .store
        .member_resume_admission(&key, now, super::CHALLENGE_TTL)
        .await?
        .ok_or_else(invalid_resume)?;
    member.secure = Some(binding.clone());
    let rooms = state.store.member_connect_rooms(&member, now).await?;
    if rooms.is_empty() {
        return Err(HumanInviteHttpError::forbidden(
            "member_no_rooms",
            "No joined rooms on this server.",
        ));
    }
    // The room select reuses the unchanged membership owner on this channel.
    // A direct redemption never issues a resume key of its own.
    let record = Arc::new(tokio::sync::Mutex::new(connect::ConnectState::redeemed(
        member,
        rooms.clone(),
        connect::ConnectOrigin::Resume,
    )));
    let (challenge_id, expires) = state.member_challenges.issue_for(
        ChallengePurpose::Connect(record),
        browser,
        epoch,
        now,
        Some(binding.clone()),
    )?;
    Ok(Json(
        json!({"rooms": rooms, "challenge_id": challenge_id, "challenge_expires_at": expires.timestamp()}),
    ))
}

#[cfg(test)]
#[path = "member_resume_web_tests.rs"]
mod tests;
