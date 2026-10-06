use super::*;
use crate::central::directory::MemberGrantPurpose;
use agentsassemble_persistence::MemberConnectRoom;
use std::sync::Arc;

pub(super) enum ConnectState {
    Issued,
    Failed,
    ConnectRedeemed {
        member: Box<MemberAdmission>,
        rooms: Vec<MemberConnectRoom>,
        selected: Option<(String, String, String, Option<Value>)>,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ChallengeRequest {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RedeemRequest {
    challenge_id: String,
    grant_token: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SelectRequest {
    #[serde(rename = "challenge_id")]
    challenge: String,
    #[serde(rename = "room_id")]
    room: String,
    #[serde(rename = "client_id")]
    client: String,
}

pub(crate) async fn start(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<Value>, HumanInviteHttpError> {
    let browser = browser(request.headers())?;
    let _: ChallengeRequest = decode_json_body(request, MAX_ADMISSION_BODY_BYTES)
        .await
        .map_err(HumanInviteHttpError::from_body)?;
    let epoch = state
        .store
        .registration_epoch()
        .await?
        .ok_or_else(invalid_challenge)?;
    let (id, expires) = state.member_challenges.issue_for(
        ChallengePurpose::Connect(Arc::new(tokio::sync::Mutex::new(ConnectState::Issued))),
        browser,
        epoch.clone(),
        Utc::now(),
    )?;
    Ok(Json(
        json!({"challenge_id":id,"challenge_hash":URL_SAFE_NO_PAD.encode(Sha256::digest(id.as_bytes())),"server_id":state.central_host_identity.server_id(),"registration_epoch":epoch,"expires_at":expires.timestamp()}),
    ))
}

async fn challenge(
    state: &AppState,
    id: &str,
    browser: [u8; 32],
) -> Result<(Challenge, Arc<tokio::sync::Mutex<ConnectState>>), HumanInviteHttpError> {
    let epoch = state
        .store
        .registration_epoch()
        .await?
        .ok_or_else(invalid_challenge)?;
    let entry = state
        .member_challenges
        .0
        .lock()
        .get(id)
        .cloned()
        .ok_or_else(invalid_challenge)?;
    if entry.browser != browser || entry.epoch != epoch || entry.expires <= Utc::now() {
        return Err(invalid_challenge());
    }
    let ChallengePurpose::Connect(record) = &entry.purpose else {
        return Err(invalid_challenge());
    };
    let record = record.clone();
    Ok((entry, record))
}

pub(crate) async fn redeem(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<Value>, HumanInviteHttpError> {
    let browser = browser(request.headers())?;
    let body: RedeemRequest = decode_json_body(request, MAX_ADMISSION_BODY_BYTES)
        .await
        .map_err(HumanInviteHttpError::from_body)?;
    let (entry, record) = challenge(&state, &body.challenge_id, browser).await?;
    let mut record = record.lock().await;
    if !matches!(*record, ConnectState::Issued) {
        return Err(invalid_challenge());
    }
    *record = ConnectState::Failed;
    let identity = state
        .central_directory
        .member_admission(
            &state.central_host_identity,
            &state.store,
            &body.grant_token,
            &URL_SAFE_NO_PAD.encode(Sha256::digest(body.challenge_id.as_bytes())),
            &entry.epoch,
            MemberGrantPurpose::Connect,
        )
        .await
        .map_err(|error| redeem_error(&error))?;
    let member = MemberAdmission {
        projection_id: identity.projection_id,
        issuer: identity.issuer,
        person_id: identity.person_id,
        display_name: identity.display_name,
        registration_epoch: entry.epoch,
        challenge_fingerprint: Sha256::digest(body.challenge_id.as_bytes()).into(),
        challenge_expires_at: entry.expires,
    };
    let rooms = state
        .store
        .member_connect_rooms(&member, Utc::now())
        .await?;
    if rooms.is_empty() {
        return Err(HumanInviteHttpError::forbidden(
            "member_no_rooms",
            "No joined rooms on this server.",
        ));
    }
    let response = json!({"rooms":rooms});
    *record = ConnectState::ConnectRedeemed {
        member: Box::new(member),
        rooms,
        selected: None,
    };
    Ok(Json(response))
}

pub(crate) async fn select(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<Value>, HumanInviteHttpError> {
    let browser = browser(request.headers())?;
    let body: SelectRequest = decode_json_body(request, MAX_ADMISSION_BODY_BYTES)
        .await
        .map_err(HumanInviteHttpError::from_body)?;
    if body.client.is_empty()
        || body.client.len() > 128
        || body.client.trim() != body.client
        || body.client.chars().any(char::is_control)
    {
        return Err(HumanInviteHttpError::bad_request(
            "client_id_required",
            "A bounded client ID is required.",
        ));
    }
    let (_, record) = challenge(&state, &body.challenge, browser).await?;
    // Lock one existing challenge across mint: exact concurrent retries await its result.
    let mut record = record.lock().await;
    let ConnectState::ConnectRedeemed {
        member,
        rooms,
        selected,
    } = &mut *record
    else {
        return Err(invalid_challenge());
    };
    if !rooms.iter().any(|r| r.room_id == body.room) {
        return Err(invalid_challenge());
    }
    if member.challenge_expires_at <= Utc::now() {
        return Err(invalid_challenge());
    }
    let (room, client, request_id, completed) = selected.get_or_insert_with(|| {
        (
            body.room.clone(),
            body.client.clone(),
            uuid::Uuid::new_v4().to_string(),
            None,
        )
    });
    if room != &body.room {
        return Err(invalid_challenge());
    }
    let decision = state
        .store
        .select_member_connect_room(member, &body.room, &browser, request_id, client, Utc::now())
        .await?;
    let HumanAdmissionDecision::Admitted(commit) = decision else {
        if let HumanAdmissionDecision::Rejected(reason) = decision {
            return Err(reason.into());
        }
        return Err(invalid_challenge());
    };
    if let Some(response) = completed {
        return Ok(Json(response.clone()));
    }
    let sessions = commit
        .replaced_session_fingerprints()
        .iter()
        .map(|f| (body.room.clone(), *f))
        .collect::<Vec<_>>();
    state.rooms.publish_session_revocations(&sessions).await;
    let (result, session_token) = commit.into_result_and_bearer();
    let bootstrap = state.store.local_bootstrap_status().await?;
    let response = serde_json::to_value(JoinResponse {
        result,
        session_token,
        server_id: bootstrap.server_id,
        authority_lineage_id: bootstrap.authority_lineage_id,
        server_product_surface: state.server_product_surface.as_ref().clone(),
    })
    .map_err(|_| invalid_challenge())?;
    *completed = Some(response.clone());
    Ok(Json(response))
}
