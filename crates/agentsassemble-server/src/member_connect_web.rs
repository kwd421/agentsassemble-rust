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

pub(super) enum ConnectState {
    Issued,
    Failed,
    ConnectRedeemed {
        member: Box<MemberAdmission>,
        rooms: Vec<MemberConnectRoom>,
        selected: HashMap<String, SelectedRoom>,
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
    *record = ConnectState::ConnectRedeemed {
        member: Box::new(member),
        rooms,
        selected: HashMap::new(),
    };
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
mod tests {
    use super::lapse_support::{LapseFixture, lapse_fixture};
    use super::{client_advertises_room_select_channel_bound, client_advertises_room_select_reuse};
    use sha2::Digest as _;

    #[test]
    fn room_select_reuse_advertisement_is_an_exact_query_pair() {
        assert!(client_advertises_room_select_reuse(Some(
            "room_select_reuse=1"
        )));
        assert!(client_advertises_room_select_reuse(Some(
            "ticket=fixture&room_select_reuse=1"
        )));
        for query in [
            None,
            Some(""),
            Some("room_select_reuse=0"),
            Some("room_select_reuse="),
            Some("room_select_reuse=10"),
            Some("room_select_reuse=true"),
            Some("xroom_select_reuse=1"),
            Some("room_select_reuse=1 "),
            Some("ticket=room_select_reuse%3D1"),
        ] {
            assert!(
                !client_advertises_room_select_reuse(query),
                "query must not opt in: {query:?}"
            );
        }
    }

    #[test]
    fn room_select_channel_bound_advertisement_is_an_exact_query_pair() {
        assert!(client_advertises_room_select_channel_bound(Some(
            "room_select_channel_bound=1"
        )));
        assert!(client_advertises_room_select_channel_bound(Some(
            "room_select_reuse=1&room_select_channel_bound=1"
        )));
        for query in [
            None,
            Some(""),
            Some("room_select_reuse=1"),
            Some("room_select_channel_bound=0"),
            Some("room_select_channel_bound="),
            Some("room_select_channel_bound=10"),
            Some("room_select_channel_bound=true"),
            Some("xroom_select_channel_bound=1"),
            Some("room_select_channel_bound=1 "),
            Some("ticket=room_select_channel_bound%3D1"),
        ] {
            assert!(
                !client_advertises_room_select_channel_bound(query),
                "query must not opt in: {query:?}"
            );
        }
    }

    #[test]
    fn legacy_redeem_query_ignores_the_new_advertisement() {
        // Mirrors the pre-gating `RedeemRequest` body shape (`deny_unknown_fields`
        // on `challenge_id`/`grant_token` only): an old host decodes the same
        // JSON body whether or not the new client appends the URL query, so the
        // opt-in never breaks an old host.
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct LegacyRedeemRequest {
            challenge_id: String,
            grant_token: String,
        }
        let body: LegacyRedeemRequest = serde_json::from_value(serde_json::json!({
            "challenge_id": "challenge",
            "grant_token": "aamc1.connect",
        }))
        .unwrap_or_else(|error| panic!("legacy redeem body: {error}"));
        assert_eq!(body.challenge_id, "challenge");
    }

    /// Host coverage for the channel-bound lapse path drives the real `select`
    /// handler (real store, real challenge map) with a fabricated secure
    /// channel standing in for the encrypted transport a plain-HTTP test
    /// harness cannot complete. Wall-clock lapse is forced by moving the
    /// stored expiries, exactly like the existing
    /// `challenge_browser_invite_epoch_expiry_claim_and_capacity_are_enforced`
    /// unit test lapses by handing `claim` an expired `now`: no clock
    /// abstraction, no production test hook, no 300 s wait. The fixture
    /// leaves central disabled (`AppState::local` without
    /// `with_central_directory`), so any successful `select` proves no new
    /// central grant was spent: `select` has no central path at all.
    pub(super) const LAPSE_TEST_EPOCH: &str = "member-channel-bound-epoch";
    pub(super) const LAPSE_TEST_ISSUER: &str = "https://central-channel-bound.test";
    pub(super) const LAPSE_TEST_PERSON: &str = "person-channel-bound";

    /// Issues a connect challenge and marks it redeemed without spending a
    /// central grant (the grant is spent only in `redeem`, which these tests
    /// bypass by installing the post-redeem state directly).
    async fn issue_redeemed(
        fixture: &LapseFixture,
        secure: Option<agentsassemble_persistence::SecureSessionBinding>,
    ) -> String {
        let (challenge_id, _) = fixture
            .state
            .member_challenges
            .issue_for(
                super::super::ChallengePurpose::Connect(std::sync::Arc::new(
                    tokio::sync::Mutex::new(super::ConnectState::Issued),
                )),
                fixture.browser,
                LAPSE_TEST_EPOCH.to_owned(),
                chrono::Utc::now(),
                secure.clone(),
            )
            .unwrap_or_else(|error| panic!("issue lapse fixture challenge: {error:?}"));
        let member = agentsassemble_persistence::MemberAdmission {
            secure,
            projection_id: "AAAAAAAAAAAAAAAAAAAAAA".into(),
            issuer: LAPSE_TEST_ISSUER.into(),
            person_id: LAPSE_TEST_PERSON.into(),
            display_name: "Member snapshot".into(),
            registration_epoch: LAPSE_TEST_EPOCH.into(),
            challenge_fingerprint: sha2::Sha256::digest(challenge_id.as_bytes()).into(),
            challenge_expires_at: chrono::Utc::now() + chrono::Duration::seconds(300),
        };
        let rooms = fixture
            .state
            .store
            .member_connect_rooms(&member, chrono::Utc::now())
            .await
            .unwrap_or_else(|error| panic!("list lapse fixture rooms: {error:?}"));
        assert!(
            !rooms.is_empty(),
            "the redeemed challenge must list joined rooms"
        );
        let record = {
            let entries = fixture.state.member_challenges.0.lock();
            let entry = entries
                .get(&challenge_id)
                .expect("issued challenge must be stored");
            match &entry.purpose {
                super::super::ChallengePurpose::Connect(record) => record.clone(),
                _ => panic!("issued challenge must be a connect challenge"),
            }
        };
        *record.lock().await = super::ConnectState::ConnectRedeemed {
            member: Box::new(member),
            rooms,
            selected: std::collections::HashMap::new(),
        };
        challenge_id
    }

    /// Moves both the stored map entry and the redeemed member window into
    /// the past, simulating a challenge that lapsed without any clock
    /// abstraction.
    async fn lapse_challenge(fixture: &LapseFixture, challenge_id: &str) {
        let past = chrono::Utc::now() - chrono::Duration::seconds(1);
        let record = {
            let mut entries = fixture.state.member_challenges.0.lock();
            let entry = entries
                .get_mut(challenge_id)
                .expect("lapsed challenge must be stored");
            entry.expires = past;
            match &entry.purpose {
                super::super::ChallengePurpose::Connect(record) => record.clone(),
                _ => panic!("lapsed challenge must be a connect challenge"),
            }
        };
        let mut guard = record.lock().await;
        match &mut *guard {
            super::ConnectState::ConnectRedeemed { member, .. } => {
                member.challenge_expires_at = past;
            }
            _ => panic!("lapsed challenge must be redeemed"),
        }
    }

    /// Issues one more challenge on the host, running the prune that drops
    /// every lapsed entry.
    fn prune_with_fresh_challenge(fixture: &LapseFixture) {
        fixture
            .state
            .member_challenges
            .issue_for(
                super::super::ChallengePurpose::Admission([1; 32]),
                [2; 32],
                LAPSE_TEST_EPOCH.to_owned(),
                chrono::Utc::now(),
                None,
            )
            .unwrap_or_else(|error| panic!("issue prune trigger challenge: {error:?}"));
    }

    fn select_request(
        fixture: &LapseFixture,
        query: &str,
        challenge_id: &str,
        room: &str,
        secure: bool,
    ) -> super::Request {
        let body = serde_json::to_vec(&serde_json::json!({
            "challenge_id": challenge_id,
            "room_id": room,
            "client_id": "channel-bound-client",
        }))
        .expect("select body must encode");
        let mut request = super::Request::builder()
            .uri(format!("/api/member-connect/select{query}"))
            .method("POST")
            .header("x-device-token", &fixture.device)
            .body(axum::body::Body::from(body))
            .expect("select request must build");
        if secure {
            request.extensions_mut().insert(fixture.secure.clone());
        }
        request
    }

    async fn select(
        fixture: &LapseFixture,
        query: &str,
        challenge_id: &str,
        room: &str,
        secure: bool,
    ) -> Result<serde_json::Value, crate::human_invite_web::HumanInviteHttpError> {
        super::select(
            axum::extract::State(fixture.state.clone()),
            select_request(fixture, query, challenge_id, room, secure),
        )
        .await
        .map(|response| response.0)
    }

    fn entry_expires(
        fixture: &LapseFixture,
        challenge_id: &str,
    ) -> Option<chrono::DateTime<chrono::Utc>> {
        fixture
            .state
            .member_challenges
            .0
            .lock()
            .get(challenge_id)
            .map(|entry| entry.expires)
    }

    #[tokio::test]
    async fn lapsed_channel_bound_select_without_secure_channel_is_refused() {
        let fixture = lapse_fixture(&[
            ("78ba8995-4ef7-4584-9d73-22caee9b3ac5", "general"),
            ("9c4b2e11-6a1e-4f3a-9d2b-2b8f2f4c5a01", "second"),
        ])
        .await;
        // Issued and redeemed over plain HTTP: no secure channel on either side.
        let challenge_id = issue_redeemed(&fixture, None).await;
        // Control: a live plain-HTTP select keeps working exactly as before.
        let live = select(&fixture, "", &challenge_id, "general", false)
            .await
            .unwrap_or_else(|error| panic!("live plain select must succeed: {error:?}"));
        assert_eq!(live["meeting_id"], "general");
        lapse_challenge(&fixture, &challenge_id).await;
        // The lapsed path with the channel-bound opt-in but still no secure
        // channel is refused: an expired redeemed challenge must not stay
        // reusable by device token + challenge id alone.
        let refused = select(
            &fixture,
            "?room_select_channel_bound=1",
            &challenge_id,
            "second",
            false,
        )
        .await;
        let error = refused
            .err()
            .unwrap_or_else(|| panic!("lapsed plain select must be refused"));
        assert_eq!(error.status, axum::http::StatusCode::UNAUTHORIZED);
        assert_eq!(error.code, "member_challenge_invalid");
        // Nothing was minted or spent: the second room was never selected and
        // the lapsed windows were left untouched.
        let record = {
            let entries = fixture.state.member_challenges.0.lock();
            let entry = entries
                .get(&challenge_id)
                .expect("refused challenge must still be stored");
            match &entry.purpose {
                super::super::ChallengePurpose::Connect(record) => record.clone(),
                _ => panic!("refused challenge must be a connect challenge"),
            }
        };
        match &*record.lock().await {
            super::ConnectState::ConnectRedeemed {
                member, selected, ..
            } => {
                assert!(selected.contains_key("general"));
                assert!(
                    !selected.contains_key("second"),
                    "the refused select must mint no session and spend no grant"
                );
                assert!(
                    member.challenge_expires_at <= chrono::Utc::now(),
                    "the refused select must not extend the member window"
                );
            }
            _ => panic!("refused challenge must stay redeemed"),
        }
        assert!(
            entry_expires(&fixture, &challenge_id)
                .is_some_and(|expires| expires <= chrono::Utc::now()),
            "the refused select must not slide the stored entry"
        );
    }

    #[tokio::test]
    async fn channel_bound_select_slides_lapsed_entry_past_later_prune() {
        let fixture = lapse_fixture(&[
            ("78ba8995-4ef7-4584-9d73-22caee9b3ac5", "general"),
            ("9c4b2e11-6a1e-4f3a-9d2b-2b8f2f4c5a01", "second"),
            ("7d3f9a21-8b4e-4c2a-9e1f-3c7d5a6b8e02", "third"),
        ])
        .await;
        let binding = fixture.secure.binding().clone();
        let challenge_id = issue_redeemed(&fixture, Some(binding)).await;
        // Live channel-bound select on the redeeming channel: no new grant.
        let first = select(
            &fixture,
            "?room_select_channel_bound=1",
            &challenge_id,
            "general",
            true,
        )
        .await
        .unwrap_or_else(|error| panic!("live bound select must succeed: {error:?}"));
        assert_eq!(first["meeting_id"], "general");
        // Advance past the window, then select again on the same live
        // channel: the lapsed path succeeds and slides the stored entry
        // forward together with the member window.
        lapse_challenge(&fixture, &challenge_id).await;
        let second = select(
            &fixture,
            "?room_select_channel_bound=1",
            &challenge_id,
            "second",
            true,
        )
        .await
        .unwrap_or_else(|error| panic!("lapsed bound select must succeed: {error:?}"));
        assert_eq!(second["meeting_id"], "second");
        assert_ne!(
            second["session_token"], first["session_token"],
            "distinct rooms mint separate sessions on one challenge"
        );
        assert!(
            entry_expires(&fixture, &challenge_id)
                .is_some_and(|expires| expires > chrono::Utc::now()),
            "the bound select must slide the stored entry past now"
        );
        // Another member issuing a challenge runs the prune over every entry:
        // the slid entry survives, so the next bound select still spends no
        // new grant.
        prune_with_fresh_challenge(&fixture);
        assert!(
            entry_expires(&fixture, &challenge_id).is_some(),
            "a channel that keeps selecting must keep its entry across prunes"
        );
        let third = select(
            &fixture,
            "?room_select_channel_bound=1",
            &challenge_id,
            "third",
            true,
        )
        .await
        .unwrap_or_else(|error| panic!("post-prune bound select must succeed: {error:?}"));
        assert_eq!(third["meeting_id"], "third");
    }

    #[tokio::test]
    async fn quiet_lapsed_entry_is_pruned_and_needs_new_grant() {
        let fixture = lapse_fixture(&[("78ba8995-4ef7-4584-9d73-22caee9b3ac5", "general")]).await;
        let binding = fixture.secure.binding().clone();
        let challenge_id = issue_redeemed(&fixture, Some(binding)).await;
        // A channel that goes quiet lapses: the next issue on this host
        // prunes the entry, and the following bound select fails closed so
        // the client takes a fresh challenge and grant.
        lapse_challenge(&fixture, &challenge_id).await;
        prune_with_fresh_challenge(&fixture);
        assert!(
            entry_expires(&fixture, &challenge_id).is_none(),
            "a quiet lapsed entry must still be pruned"
        );
        let refused = select(
            &fixture,
            "?room_select_channel_bound=1",
            &challenge_id,
            "general",
            true,
        )
        .await;
        let error = refused
            .err()
            .unwrap_or_else(|| panic!("select on a pruned challenge must be refused"));
        assert_eq!(error.status, axum::http::StatusCode::UNAUTHORIZED);
        assert_eq!(error.code, "member_challenge_invalid");
    }

    async fn age_completed(fixture: &LapseFixture, challenge_id: &str, room: &str) {
        let record = {
            let entries = fixture.state.member_challenges.0.lock();
            let entry = entries.get(challenge_id).expect("aged challenge must be stored");
            match &entry.purpose {
                super::super::ChallengePurpose::Connect(record) => record.clone(),
                _ => panic!("aged challenge must be a connect challenge"),
            }
        };
        let mut guard = record.lock().await;
        match &mut *guard {
            super::ConnectState::ConnectRedeemed { selected, .. } => {
                let entry = selected.get_mut(room).expect("aged room must have been selected");
                entry.completed_at = Some(
                    chrono::Utc::now()
                        - super::SELECT_COMPLETED_RETRY_WINDOW
                        - chrono::Duration::seconds(1),
                );
            }
            _ => panic!("aged challenge must stay redeemed"),
        }
    }

    fn bearer_fingerprint(token: &serde_json::Value) -> [u8; 32] {
        sha2::Sha256::digest(
            token.as_str().expect("select must mint a session token").as_bytes(),
        )
        .into()
    }

    async fn kick_general(fixture: &LapseFixture, participant: &str) {
        use agentsassemble_domain::{
            AuthenticatedPrincipal, CapabilitySet, ClientKind, InviteScope,
        };
        let manager = AuthenticatedPrincipal {
            room_id: "general".into(),
            principal_id: agentsassemble_domain::LOCAL_OPERATOR_USER_ID.into(),
            participant_id: agentsassemble_domain::LOCAL_OPERATOR_PARTICIPANT_ID.into(),
            display_name: "SeiNel".into(),
            client_kind: ClientKind::Browser,
            is_operator: true,
            capabilities: CapabilitySet::local_operator(ClientKind::Browser, InviteScope::ReadWrite),
            invite_scope: InviteScope::ReadWrite,
        };
        fixture
            .state
            .store
            .execute_participant_removal(
                agentsassemble_persistence::RoomMutationAuthority::TrustedPrincipal(&manager),
                "retry-window-kick",
                "participant.kick",
                &serde_json::json!({"participant_id": participant}),
            )
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn past_retry_window_reselect_mints_new_bearer_and_revokes_previous() {
        let fixture = lapse_fixture(&[("78ba8995-4ef7-4584-9d73-22caee9b3ac5", "general")]).await;
        let binding = fixture.secure.binding().clone();
        let challenge_id = issue_redeemed(&fixture, Some(binding)).await;
        let query = "?room_select_channel_bound=1";
        let first = select(&fixture, query, &challenge_id, "general", true).await.unwrap();
        age_completed(&fixture, &challenge_id, "general").await;
        let second = select(&fixture, query, &challenge_id, "general", true).await.unwrap();
        assert_ne!(first["session_token"], second["session_token"]);
        assert!(
            fixture
                .state
                .store
                .authorize_human_session(&bearer_fingerprint(&first["session_token"]))
                .await
                .is_err()
        );
        fixture
            .state
            .store
            .authorize_human_session(&bearer_fingerprint(&second["session_token"]))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn past_retry_window_reselect_after_kick_is_refused() {
        let fixture = lapse_fixture(&[("78ba8995-4ef7-4584-9d73-22caee9b3ac5", "general")]).await;
        let binding = fixture.secure.binding().clone();
        let challenge_id = issue_redeemed(&fixture, Some(binding)).await;
        let query = "?room_select_channel_bound=1";
        let first = select(&fixture, query, &challenge_id, "general", true).await.unwrap();
        kick_general(&fixture, first["agent_id"].as_str().expect("select must report a participant"))
            .await;
        age_completed(&fixture, &challenge_id, "general").await;
        let error = select(&fixture, query, &challenge_id, "general", true)
            .await
            .err()
            .expect("past-window reselect after kick must be refused");
        assert!(error.code == "member_membership_ended" || error.code == "session_unavailable");
    }

    #[tokio::test]
    async fn exact_retry_inside_retry_window_replays_identical_response() {
        let fixture = lapse_fixture(&[("78ba8995-4ef7-4584-9d73-22caee9b3ac5", "general")]).await;
        let binding = fixture.secure.binding().clone();
        let challenge_id = issue_redeemed(&fixture, Some(binding)).await;
        let query = "?room_select_channel_bound=1";
        let first = select(&fixture, query, &challenge_id, "general", true).await.unwrap();
        let retry = select(&fixture, query, &challenge_id, "general", true).await.unwrap();
        assert_eq!(first, retry);
    }
}
