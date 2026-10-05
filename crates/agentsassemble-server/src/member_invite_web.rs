use super::{
    AppState, DEVICE_CREDENTIAL_HEADER, Deserialize, HeaderMap, HumanAdmissionDecision,
    HumanAdmissionInput, HumanInviteHttpError, HumanInvitePreflight, HumanInvitePreflightRejection,
    JoinResponse, Json, MAX_ADMISSION_BODY_BYTES, PreflightRequest, PreparedHumanAdmission,
    Request, State, StatusCode, Value, authenticated_invite_evidence, decode_json_body,
    fingerprint_browser_credential, json, required_header,
};
use agentsassemble_persistence::{HumanInvitePreflightRequest, MemberAdmission};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Duration, Utc};
use parking_lot::Mutex;
use sha2::{Digest, Sha256};
use std::collections::HashMap;

const CHALLENGE_LIMIT: usize = 1024;
const CHALLENGE_TTL: Duration = Duration::seconds(300);

#[derive(Default)]
pub(crate) struct MemberChallenges(Mutex<HashMap<String, Challenge>>);

#[derive(Clone)]
struct Challenge {
    invite: [u8; 32],
    browser: [u8; 32],
    epoch: String,
    expires: DateTime<Utc>,
    redeeming: bool,
    completed: Option<([u8; 32], MemberAdmission)>,
}

impl MemberChallenges {
    fn issue(
        &self,
        invite: [u8; 32],
        browser: [u8; 32],
        epoch: String,
        now: DateTime<Utc>,
    ) -> Result<(String, DateTime<Utc>), HumanInviteHttpError> {
        let mut entries = self.0.lock();
        entries.retain(|_, challenge| challenge.expires > now);
        if entries.len() >= CHALLENGE_LIMIT {
            return Err(HumanInviteHttpError::new(
                StatusCode::TOO_MANY_REQUESTS,
                "member_challenge_capacity",
                "Retry after the pending challenges expire.",
            ));
        }
        // Two independent random UUIDs supply 244 random bits, without raw credentials.
        let id = format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        let expires = now + CHALLENGE_TTL;
        entries.insert(
            id.clone(),
            Challenge {
                invite,
                browser,
                epoch,
                expires,
                redeeming: false,
                completed: None,
            },
        );
        Ok((id, expires))
    }

    fn claim(
        &self,
        id: &str,
        invite: [u8; 32],
        browser: [u8; 32],
        epoch: &str,
        now: DateTime<Utc>,
        request_hash: [u8; 32],
    ) -> Result<Challenge, HumanInviteHttpError> {
        let mut entries = self.0.lock();
        let challenge = entries.get_mut(id).ok_or_else(invalid_challenge)?;
        if (challenge.redeeming
            && challenge
                .completed
                .as_ref()
                .is_none_or(|(hash, _)| *hash != request_hash))
            || challenge.expires <= now
            || challenge.invite != invite
            || challenge.browser != browser
            || challenge.epoch != epoch
        {
            return Err(invalid_challenge());
        }
        challenge.redeeming = true;
        Ok(challenge.clone())
    }
}

fn invalid_challenge() -> HumanInviteHttpError {
    HumanInviteHttpError::unauthorized(
        "member_challenge_invalid",
        "Start again with a fresh member challenge and grant.",
    )
}

fn browser(headers: &HeaderMap) -> Result<[u8; 32], HumanInviteHttpError> {
    fingerprint_browser_credential(required_header(headers, &DEVICE_CREDENTIAL_HEADER)?)
        .ok_or_else(invalid_challenge)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MemberJoinRequest {
    invite_token: String,
    challenge_id: String,
    grant_token: String,
    request_id: String,
    client_id: String,
}

pub(super) async fn start(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<Value>, HumanInviteHttpError> {
    let browser = browser(request.headers())?;
    let body: PreflightRequest = decode_json_body(request, MAX_ADMISSION_BODY_BYTES)
        .await
        .map_err(HumanInviteHttpError::from_body)?;
    let credential =
        authenticated_invite_evidence(&state.human_invite_credentials, body.invite_token.trim())
            .map_err(|_| invalid_challenge())?;
    let now = Utc::now();
    let preflight = state
        .store
        .preflight_human_invite(&HumanInvitePreflightRequest {
            credential,
            browser_credential_fingerprint: None,
            session_fingerprint: None,
            now,
        })
        .await?;
    // A spent invite can still identify a committed (binding, invite) retry.
    // Central identity is unknown until redemption; the transaction decides admission.
    if let HumanInvitePreflight::Rejected(reason) = preflight
        && reason != HumanInvitePreflightRejection::InviteUseLimitReached
    {
        return Err(HumanInviteHttpError::forbidden(
            "invite_invalid",
            "Invite or room is unavailable.",
        ));
    }
    let epoch = state
        .store
        .registration_epoch()
        .await?
        .ok_or_else(invalid_challenge)?;
    let (id, expires) = state.member_challenges.issue(
        Sha256::digest(body.invite_token.trim().as_bytes()).into(),
        browser,
        epoch.clone(),
        now,
    )?;
    Ok(Json(
        json!({"challenge_id": id, "challenge_hash": URL_SAFE_NO_PAD.encode(Sha256::digest(id.as_bytes())),
        "server_id": state.central_host_identity.server_id(), "registration_epoch": epoch, "expires_at": expires.timestamp()}),
    ))
}

pub(super) async fn join(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<JoinResponse>, HumanInviteHttpError> {
    let browser = browser(request.headers())?;
    let body: MemberJoinRequest = decode_json_body(request, MAX_ADMISSION_BODY_BYTES)
        .await
        .map_err(HumanInviteHttpError::from_body)?;
    let credential =
        authenticated_invite_evidence(&state.human_invite_credentials, body.invite_token.trim())
            .map_err(|_| invalid_challenge())?;
    let prepared = PreparedHumanAdmission::prepare(
        credential,
        browser,
        &HumanAdmissionInput {
            request_id: body.request_id.clone(),
            meeting_id_assertion: String::new(),
            display_name: String::new(),
            participant_type: "human".into(),
            owner_display_name: String::new(),
            client_id: body.client_id.clone(),
            avatar_image_url: String::new(),
        },
    )
    .map_err(HumanInviteHttpError::from_input)?;
    if prepared.client_id().is_empty() {
        return Err(HumanInviteHttpError::bad_request(
            "client_id_required",
            "client_id is required.",
        ));
    }
    let response_request_id = prepared.request_id().to_string();
    let response_client_id = prepared.client_id().to_owned();
    let request_hash: [u8; 32] = Sha256::digest(
        json!([body.grant_token, body.request_id, body.client_id])
            .to_string()
            .as_bytes(),
    )
    .into();
    let member = redeem_member(&state, &body, browser, request_hash).await?;
    let prepared = prepared.with_member(member.clone());
    match state.rooms.admit_human(prepared).await? {
        HumanAdmissionDecision::Admitted(commit) => {
            if let Some(challenge) = state.member_challenges.0.lock().get_mut(&body.challenge_id) {
                challenge.completed = Some((request_hash, member));
            }
            let (mut result, session_token) = commit.into_result_and_bearer();
            // Correlate this HTTP response without changing the durable canonical result.
            result.request_id = response_request_id;
            result.client_id = response_client_id;
            let bootstrap = state.store.local_bootstrap_status().await?;
            Ok(Json(JoinResponse {
                result,
                session_token,
                server_id: bootstrap.server_id,
                authority_lineage_id: bootstrap.authority_lineage_id,
                server_product_surface: state.server_product_surface.as_ref().clone(),
            }))
        }
        HumanAdmissionDecision::Rejected(reason) => Err(reason.into()),
    }
}

async fn redeem_member(
    state: &AppState,
    body: &MemberJoinRequest,
    browser: [u8; 32],
    request_hash: [u8; 32],
) -> Result<MemberAdmission, HumanInviteHttpError> {
    let epoch = state
        .store
        .registration_epoch()
        .await?
        .ok_or_else(invalid_challenge)?;
    let challenge = state.member_challenges.claim(
        &body.challenge_id,
        Sha256::digest(body.invite_token.trim().as_bytes()).into(),
        browser,
        &epoch,
        Utc::now(),
        request_hash,
    )?;
    if let Some((_, member)) = challenge.completed {
        return Ok(member);
    }
    let hash = URL_SAFE_NO_PAD.encode(Sha256::digest(body.challenge_id.as_bytes()));
    let redeemed = state
        .central_directory
        .member_admission(
            &state.central_host_identity,
            &state.store,
            &body.grant_token,
            &hash,
            &epoch,
        )
        .await;
    let identity = redeemed.map_err(|_| {
        // Failed/unknown redemption cannot be retried; only committed success is retained.
        state.member_challenges.0.lock().remove(&body.challenge_id);
        HumanInviteHttpError::new(
            StatusCode::BAD_GATEWAY,
            "member_redeem_failed",
            "Member verification failed. Start again with a fresh challenge and grant.",
        )
    })?;
    Ok(MemberAdmission {
        issuer: identity.issuer,
        person_id: identity.person_id,
        display_name: identity.display_name,
        registration_epoch: epoch,
        challenge_expires_at: challenge.expires,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn challenge_browser_invite_epoch_expiry_claim_and_capacity_are_enforced() {
        let challenges = MemberChallenges::default();
        let now = Utc::now();
        let (id, expires) = challenges
            .issue([1; 32], [2; 32], "epoch".into(), now)
            .unwrap_or_else(|_| panic!("issue"));
        for (invite, browser, epoch) in [
            ([3; 32], [2; 32], "epoch"),
            ([1; 32], [3; 32], "epoch"),
            ([1; 32], [2; 32], "other"),
        ] {
            assert!(
                challenges
                    .claim(&id, invite, browser, epoch, now, [0; 32])
                    .is_err()
            );
        }
        assert!(
            challenges
                .claim(&id, [1; 32], [2; 32], "epoch", expires, [0; 32])
                .is_err()
        );
        assert!(
            challenges
                .claim(&id, [1; 32], [2; 32], "epoch", now, [0; 32])
                .is_ok()
        );
        assert!(
            challenges
                .claim(&id, [1; 32], [2; 32], "epoch", now, [0; 32])
                .is_err()
        );
        for _ in 1..CHALLENGE_LIMIT {
            assert!(
                challenges
                    .issue([1; 32], [2; 32], "epoch".into(), now)
                    .is_ok()
            );
        }
        assert!(
            challenges
                .issue([1; 32], [2; 32], "epoch".into(), now)
                .is_err()
        );
        assert!(
            challenges
                .issue([1; 32], [2; 32], "epoch".into(), expires)
                .is_ok()
        );
    }

    #[tokio::test]
    async fn concurrent_challenge_claim_has_one_winner() {
        let challenges = std::sync::Arc::new(MemberChallenges::default());
        let now = Utc::now();
        let (id, _) = challenges
            .issue([1; 32], [2; 32], "epoch".into(), now)
            .unwrap_or_else(|_| panic!("issue"));
        let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(2));
        let mut tasks = Vec::new();
        for _ in 0..2 {
            let challenges = challenges.clone();
            let id = id.clone();
            let barrier = barrier.clone();
            tasks.push(tokio::spawn(async move {
                barrier.wait().await;
                challenges
                    .claim(&id, [1; 32], [2; 32], "epoch", now, [0; 32])
                    .is_ok()
            }));
        }
        let mut wins = 0;
        for task in tasks {
            if task.await.unwrap_or_else(|_| panic!("join")) {
                wins += 1;
            }
        }
        assert_eq!(wins, 1);
    }
}
