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
    assert_eq!(body.grant_token, "aamc1.connect");
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
            super::super::ChallengePurpose::Connect(std::sync::Arc::new(tokio::sync::Mutex::new(
                super::ConnectState::Issued,
            ))),
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
    *record.lock().await =
        super::ConnectState::redeemed(member, rooms, super::ConnectOrigin::Central);
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
        entry_expires(&fixture, &challenge_id).is_some_and(|expires| expires <= chrono::Utc::now()),
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
        entry_expires(&fixture, &challenge_id).is_some_and(|expires| expires > chrono::Utc::now()),
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
        let entry = entries
            .get(challenge_id)
            .expect("aged challenge must be stored");
        match &entry.purpose {
            super::super::ChallengePurpose::Connect(record) => record.clone(),
            _ => panic!("aged challenge must be a connect challenge"),
        }
    };
    let mut guard = record.lock().await;
    match &mut *guard {
        super::ConnectState::ConnectRedeemed { selected, .. } => {
            let entry = selected
                .get_mut(room)
                .expect("aged room must have been selected");
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
        token
            .as_str()
            .expect("select must mint a session token")
            .as_bytes(),
    )
    .into()
}

async fn kick_general(fixture: &LapseFixture, participant: &str) {
    use agentsassemble_domain::{AuthenticatedPrincipal, CapabilitySet, ClientKind, InviteScope};
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
    let first = select(&fixture, query, &challenge_id, "general", true)
        .await
        .unwrap();
    age_completed(&fixture, &challenge_id, "general").await;
    let second = select(&fixture, query, &challenge_id, "general", true)
        .await
        .unwrap();
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
    let first = select(&fixture, query, &challenge_id, "general", true)
        .await
        .unwrap();
    kick_general(
        &fixture,
        first["agent_id"]
            .as_str()
            .expect("select must report a participant"),
    )
    .await;
    age_completed(&fixture, &challenge_id, "general").await;
    let error = select(&fixture, query, &challenge_id, "general", true)
        .await
        .expect_err("past-window reselect after kick must be refused");
    assert!(error.code == "member_membership_ended" || error.code == "session_unavailable");
}

#[tokio::test]
async fn exact_retry_inside_retry_window_replays_identical_response() {
    let fixture = lapse_fixture(&[("78ba8995-4ef7-4584-9d73-22caee9b3ac5", "general")]).await;
    let binding = fixture.secure.binding().clone();
    let challenge_id = issue_redeemed(&fixture, Some(binding)).await;
    let query = "?room_select_channel_bound=1";
    let first = select(&fixture, query, &challenge_id, "general", true)
        .await
        .unwrap();
    let retry = select(&fixture, query, &challenge_id, "general", true)
        .await
        .unwrap();
    assert_eq!(first, retry);
}
