use super::*;
use crate::central::directory::MemberGrantPurpose;
use agentsassemble_persistence::MemberConnectRoom;
use std::collections::HashMap;
use std::sync::Arc;

/// One redeemed connect challenge remains usable for every joined room on the
/// same admitted channel until its 300-second window closes, so a member
/// device spends exactly one central grant per host admission instead of one
/// per room select. Each room keeps its own minted session and is keyed by
/// room so exact retries replay one response while distinct rooms mint
/// separate sessions. The completed replay is only a short network-retry
/// idempotency window (see `SELECT_COMPLETED_RETRY_WINDOW`): past it a
/// re-select must mint again so a dead bearer is never replayed forever.
pub(super) struct SelectedRoom {
    client: String,
    request_id: String,
    completed: Option<Value>,
    completed_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// A cached `completed` select response is replayed only for an exact
/// concurrent/network retry inside this window. Past it the same room select
/// mints a fresh session with a fresh request id (rechecking join/room/
/// admission/epoch in the mint transaction; the commit's replaced-session
/// revocation already ends the previous bearer).
const SELECT_COMPLETED_RETRY_WINDOW: chrono::Duration = chrono::Duration::seconds(30);

/// URL query pair through which a client build advertises that it tolerates
/// the optional `room_select_reuse` redeem field. Old hosts never read the
/// query beyond the JSON body (`RedeemRequest` keeps `deny_unknown_fields`,
/// so a body field would be rejected there), so the advertisement rides the
/// URL exactly like the `channel_message_cursors=1` precedent and is ignored
/// there. Old clients keep a strict `["rooms"]` exact-keys check, so the host
/// only ever sends the capability to a client that advertised it.
pub(crate) const ROOM_SELECT_REUSE_QUERY: &str = "room_select_reuse";
const ROOM_SELECT_REUSE_VALUE: &str = "1";

pub(crate) fn client_advertises_room_select_reuse(query: Option<&str>) -> bool {
    query.is_some_and(|query| {
        query.split('&').any(|pair| {
            pair.split_once('=')
                == Some((ROOM_SELECT_REUSE_QUERY, ROOM_SELECT_REUSE_VALUE))
        })
    })
}

/// URL query pair through which a client build advertises that it selects on
/// a live admitted channel past the 300-second challenge window without a new
/// central grant. Same gating as `room_select_reuse`: old hosts ignore the
/// query, and the host only ever sends the capability field to a client that
/// advertised it, so old clients keep their strict exact-keys shape.
pub(crate) const ROOM_SELECT_CHANNEL_BOUND_QUERY: &str = "room_select_channel_bound";
const ROOM_SELECT_CHANNEL_BOUND_VALUE: &str = "1";

pub(crate) fn client_advertises_room_select_channel_bound(query: Option<&str>) -> bool {
    query.is_some_and(|query| {
        query.split('&').any(|pair| {
            pair.split_once('=')
                == Some((
                    ROOM_SELECT_CHANNEL_BOUND_QUERY,
                    ROOM_SELECT_CHANNEL_BOUND_VALUE,
                ))
        })
    })
}

/// Where a redeemed connect challenge came from: a central grant may issue
/// the admission's resume key once, a direct resume never may.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ConnectOrigin {
    Central,
    Resume,
}

pub(super) enum ConnectState {
    Issued,
    Failed,
    ConnectRedeemed {
        member: Box<MemberAdmission>,
        rooms: Vec<MemberConnectRoom>,
        selected: HashMap<String, SelectedRoom>,
        /// Where this redemption came from, and whether its key was issued.
        origin: ConnectOrigin,
        resume_key_issued: bool,
    },
}

impl ConnectState {
    /// The redeemed connect state of one admitted member on one channel.
    pub(super) fn redeemed(
        member: MemberAdmission,
        rooms: Vec<MemberConnectRoom>,
        origin: ConnectOrigin,
    ) -> Self {
        Self::ConnectRedeemed {
            member: Box::new(member),
            rooms,
            selected: HashMap::new(),
            origin,
            resume_key_issued: false,
        }
    }

    /// Takes the one-shot claim of this redemption's resume key: a central
    /// grant may issue its key once, a direct resume never may.
    pub(super) fn claim_resume_key(&mut self) -> Option<MemberAdmission> {
        let ConnectState::ConnectRedeemed {
            member,
            origin,
            resume_key_issued,
            ..
        } = &mut *self
        else {
            return None;
        };
        if *origin != ConnectOrigin::Central || *resume_key_issued {
            return None;
        }
        *resume_key_issued = true;
        Some((**member).clone())
    }
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
    let secure_client = crate::secure_client::from_request(&request);
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
        secure_client
            .as_ref()
            .map(|client| client.binding().clone()),
    )?;
    Ok(Json(
        json!({"challenge_id":id,"challenge_hash":URL_SAFE_NO_PAD.encode(Sha256::digest(id.as_bytes())),"server_id":state.central_host_identity.server_id(),"registration_epoch":epoch,"expires_at":expires.timestamp()}),
    ))
}

async fn challenge(
    state: &AppState,
    id: &str,
    browser: [u8; 32],
    client: Option<&crate::secure_client::SecureClient>,
    lapsed_channel_bound: bool,
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
    if entry.secure.as_ref() != client.map(crate::secure_client::SecureClient::binding)
        || entry.browser != browser
        || entry.epoch != epoch
    {
        return Err(invalid_challenge());
    }
    if entry.expires <= Utc::now() {
        // The 300-second window lapsed. A select on the same live secure
        // channel that redeemed the challenge stays usable without a new
        // central grant: the channel binding already matched above, so only
        // the redeemed state remains to verify. Every other admission check
        // (epoch, membership, room, scope) still runs at mint.
        if !lapsed_channel_bound {
            return Err(invalid_challenge());
        }
        // The lapsed path stays bound to a present secure channel. The
        // binding check above also passes when both sides are absent, but a
        // challenge issued over plain HTTP (entry.secure == None) selected
        // over plain HTTP (client == None) must keep the exact old 300 s
        // wall: otherwise an expired redeemed challenge would stay reusable
        // forever by anyone holding only the device token + challenge id.
        if !matches!((&entry.secure, client), (Some(expected), Some(present)) if expected == present.binding())
        {
            return Err(invalid_challenge());
        }
        let record = match &entry.purpose {
            ChallengePurpose::Connect(record) => record.clone(),
            _ => return Err(invalid_challenge()),
        };
        if !matches!(*record.lock().await, ConnectState::ConnectRedeemed { .. }) {
            return Err(invalid_challenge());
        }
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
    let secure_client = crate::secure_client::from_request(&request);
    let browser = browser(request.headers())?;
    // Read the capability advertisement before the body move consumes the
    // request; the query is ignored by old hosts and old clients alike.
    let reuse_advertised = client_advertises_room_select_reuse(request.uri().query());
    let channel_bound_advertised =
        client_advertises_room_select_channel_bound(request.uri().query());
    let body: RedeemRequest = decode_json_body(request, MAX_ADMISSION_BODY_BYTES)
        .await
        .map_err(HumanInviteHttpError::from_body)?;
    let (entry, record) = challenge(
        &state,
        &body.challenge_id,
        browser,
        secure_client.as_ref(),
        false,
    )
    .await?;
    let mut record = record.lock().await;
    if !matches!(*record, ConnectState::Issued) {
        return Err(invalid_challenge());
    }
    *record = ConnectState::Failed;
    let identity = state
        .central_directory
        .member_admission(
            crate::central::directory::RedeemHost {
                identity: &state.central_host_identity,
                store: &state.store,
            },
            &body.grant_token,
            &URL_SAFE_NO_PAD.encode(Sha256::digest(body.challenge_id.as_bytes())),
            &entry.epoch,
            MemberGrantPurpose::Connect,
            secure_client.as_ref(),
        )
        .await
        .map_err(|error| redeem_error(&error))?;
    let member = MemberAdmission {
        secure: secure_client
            .as_ref()
            .map(|client| client.binding().clone()),
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
    // Capability advertisement, gated on the client opt-in: only a client
    // that requested `?room_select_reuse=1` can parse the extra field, so an
    // older strict `["rooms"]` client keeps joining on the exact old shape.
    // This host keeps one redeemed connect challenge usable across every
    // joined room on the channel, so the client never mints another central
    // grant for a select on a live admission. An older host omits the field
    // and keeps the per-room grant path. The channel-bound field rides its own
    // opt-in pair for the same reason: a client that only opted into reuse
    // keeps its strict shape.
    let response = if reuse_advertised {
        if channel_bound_advertised {
            json!({"rooms":rooms, "room_select_reuse":true, "room_select_channel_bound":true})
        } else {
            json!({"rooms":rooms, "room_select_reuse":true})
        }
    } else if channel_bound_advertised {
        json!({"rooms":rooms, "room_select_channel_bound":true})
    } else {
        json!({"rooms":rooms})
    };
    *record = ConnectState::redeemed(member, rooms, ConnectOrigin::Central);
    Ok(Json(response))
}

pub(crate) async fn select(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<Value>, HumanInviteHttpError> {
    let secure_client = crate::secure_client::from_request(&request);
    let browser = browser(request.headers())?;
    // The channel-bound opt-in rides the URL query like the reuse precedent:
    // old hosts ignore it and keep the exact old 300-second wall for every
    // select, while a new host lets a select on the same live secure channel
    // that redeemed the challenge proceed past the window. Read before the
    // body move consumes the request.
    let channel_bound = client_advertises_room_select_channel_bound(request.uri().query());
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
    let (entry, record) = challenge(
        &state,
        &body.challenge,
        browser,
        secure_client.as_ref(),
        channel_bound,
    )
    .await?;
    // Lock one existing challenge across mint: exact concurrent retries await its result.
    let mut record = record.lock().await;
    let ConnectState::ConnectRedeemed {
        member,
        rooms,
        selected,
        ..
    } = &mut *record
    else {
        return Err(invalid_challenge());
    };
    // The reused challenge is bound to this channel by `challenge`; the room
    // must still be one this admitted person is joined to and the admission
    // window must still be open.
    if !rooms.iter().any(|r| r.room_id == body.room) {
        return Err(invalid_challenge());
    }
    // A channel-bound select is only ever a bound-channel select: `challenge`
    // already required a present secure client matching the stored binding on
    // the lapsed path, and the same equality holds on the live path. Plain
    // HTTP (both sides absent) never takes the sliding path below.
    let bound_channel = matches!((&entry.secure, secure_client.as_ref()), (Some(expected), Some(present)) if expected == present.binding());
    let now = Utc::now();
    if member.challenge_expires_at <= now {
        if !channel_bound || !bound_channel {
            return Err(invalid_challenge());
        }
        // Same live channel that redeemed the challenge (verified in
        // `challenge` above): extend the window so the mint rechecks join,
        // room, admission and epoch exactly as at mint instead of spending a
        // new central grant. The channel itself is the expiry from here on:
        // it cannot present once closed, and a new channel needs its own
        // challenge and grant.
        member.challenge_expires_at = member.challenge_expires_at.max(now + super::CHALLENGE_TTL);
    }
    if channel_bound && bound_channel {
        // Keep the stored map entry alive alongside the member window so a
        // prune triggered by any later challenge issue (any member, any
        // admission) does not drop a channel that keeps selecting. The slide
        // only ever moves the window forward to now + CHALLENGE_TTL; a
        // channel that goes quiet for >300 s still lapses and needs a new
        // grant.
        member.challenge_expires_at = member.challenge_expires_at.max(now + super::CHALLENGE_TTL);
        state
            .member_challenges
            .slide_connect_window(&body.challenge, now);
    }
    let (client, request_id) = {
        let entry = selected
            .entry(body.room.clone())
            .or_insert_with(|| SelectedRoom {
                client: body.client.clone(),
                request_id: uuid::Uuid::new_v4().to_string(),
                completed: None,
                completed_at: None,
            });
        // One room keeps one client identity on this challenge; a different
        // client presenting it is a stale/foreign request.
        if entry.client != body.client {
            return Err(invalid_challenge());
        }
        if let Some(response) = &entry.completed {
            let fresh_retry = entry
                .completed_at
                .is_some_and(|at| now.signed_duration_since(at) < SELECT_COMPLETED_RETRY_WINDOW);
            if fresh_retry {
                return Ok(Json(response.clone()));
            }
            // Past the retry window the cached bearer may be dead (session
            // TTL, kick/leave/ban) and join/epoch must be rechecked: mint
            // again with a fresh request id exactly as at first mint. The
            // commit's replaced-session revocation ends the previous bearer.
            entry.request_id = uuid::Uuid::new_v4().to_string();
            entry.completed = None;
            entry.completed_at = None;
        }
        (entry.client.clone(), entry.request_id.clone())
    };
    // Every select rechecks the canonical join, room state and admission
    // currency inside the mint transaction: kick/leave/ban/removal/epoch still
    // refuse here even though no new central grant is spent.
    let decision = state
        .store
        .select_member_connect_room(
            member,
            &body.room,
            &browser,
            &request_id,
            &client,
            Utc::now(),
        )
        .await?;
    let HumanAdmissionDecision::Admitted(commit) = decision else {
        if let HumanAdmissionDecision::Rejected(reason) = decision {
            return Err(reason.into());
        }
        return Err(invalid_challenge());
    };
    let sessions = commit
        .replaced_session_fingerprints()
        .iter()
        .map(|f| (body.room.clone(), *f))
        .collect::<Vec<_>>();
    state.rooms.publish_session_revocations(&sessions).await;
    state.rooms.notify_committed_events(commit.events()).await;
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
    if let Some(entry) = selected.get_mut(&body.room) {
        entry.completed = Some(response.clone());
        entry.completed_at = Some(Utc::now());
    }
    if let Some(client) = &secure_client {
        client.adopt(&state, None)?;
    }
    Ok(Json(response))
}

#[cfg(test)]
#[path = "member_connect_web_lapse_support.rs"]
mod lapse_support;

#[cfg(test)]
#[path = "member_connect_web_tests.rs"]
mod tests;
