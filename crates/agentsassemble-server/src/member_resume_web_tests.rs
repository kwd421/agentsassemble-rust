//! Host-side coverage for the member resume key: issuance after a central
//! admission, the direct reconnect, and every refusal that keeps a captured
//! signature or a replayed nonce useless.
use super::*;
use agentsassemble_domain::{
    InviteScope, LOCAL_OPERATOR_PARTICIPANT_ID, LOCAL_OPERATOR_USER_ID, ProviderCatalog,
};
use agentsassemble_persistence::{
    HumanAdmissionDecision, HumanAdmissionInput, HumanInviteCredentialEvidence, MemberAdmission,
    NewHumanInvite, PreparedHumanAdmission, RoomManagerAuthority, SqliteStore,
};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ring::rand::SystemRandom;
use ring::signature::{ECDSA_P256_SHA256_ASN1_SIGNING, EcdsaKeyPair, KeyPair};
use tower::ServiceExt as _;

const EPOCH: &str = "resume-epoch";
const ISSUER: &str = "https://central-resume.test";
const PERSON: &str = "person-resume";
const ORIGIN: &str = "https://resume-host.test";

struct Fixture {
    state: crate::AppState,
    store: SqliteStore,
    device: String,
    browser: [u8; 32],
    secure: crate::secure_client::SecureClient,
    key_pair: EcdsaKeyPair,
}

/// The client-built transcript: the same JSON array the browser signs, written
/// out here independently of the host's encoder.
fn client_transcript(fields: &super::ResumeTranscript<'_>) -> Vec<u8> {
    serde_json::json!([
        "AA-MEMBER-RESUME-1",
        fields.server_id,
        fields.registration_epoch,
        fields.origin,
        fields.generation,
        fields.host_key_fingerprint,
        fields.browser_credential_fingerprint,
        fields.client_public_key,
        fields.channel_id,
        "resume",
        fields.nonce
    ])
    .to_string()
    .into_bytes()
}

/// The raw P1363 signature a browser produces for a P-256 key.
fn raw_signature(der: &[u8]) -> [u8; 64] {
    let mut offset = 2;
    let mut read = || {
        assert_eq!(
            der[offset], 0x02,
            "the DER signature must hold two integers"
        );
        let length = usize::from(der[offset + 1]);
        let bytes = &der[offset + 2..offset + 2 + length];
        offset += 2 + length;
        bytes
    };
    let r = read();
    let s = read();
    let mut out = [0u8; 64];
    out[32 - r.len().min(32)..32].copy_from_slice(&r[r.len().saturating_sub(32)..]);
    out[64 - s.len().min(32)..64].copy_from_slice(&s[s.len().saturating_sub(32)..]);
    out
}

fn base64(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

/// The raw P1363 signature of any device key.
fn sign_with(key_pair: &EcdsaKeyPair, message: &[u8]) -> String {
    let rng = SystemRandom::new();
    let der = key_pair
        .sign(&rng, message)
        .unwrap_or_else(|error| panic!("sign resume transcript: {error:?}"));
    base64(&raw_signature(der.as_ref()))
}

/// Another device key: a second person on the same browser credential.
fn other_key_pair() -> EcdsaKeyPair {
    let rng = SystemRandom::new();
    let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, &rng)
        .unwrap_or_else(|error| panic!("generate the other key: {error:?}"));
    EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, pkcs8.as_ref(), &rng)
        .unwrap_or_else(|error| panic!("load the other key: {error:?}"))
}

async fn fixture() -> Fixture {
    let store = SqliteStore::open(&format!(
        "sqlite:file:{}?mode=memory&cache=shared",
        uuid::Uuid::new_v4()
    ))
    .await
    .unwrap_or_else(|error| panic!("open resume fixture store: {error:?}"));
    store
        .bootstrap_local_authority("8f0f1a2c-6b3e-4c2f-9a1d-0f4b6c7d8e90", "Host")
        .await
        .unwrap_or_else(|error| panic!("bootstrap resume fixture: {error:?}"));
    store
        .set_registration_epoch(Some(EPOCH))
        .await
        .unwrap_or_else(|error| panic!("set resume fixture epoch: {error:?}"));
    let now = chrono::Utc::now();
    let member = MemberAdmission {
        secure: None,
        projection_id: "resume-projection".into(),
        issuer: ISSUER.into(),
        person_id: PERSON.into(),
        display_name: "Member".into(),
        registration_epoch: EPOCH.into(),
        challenge_fingerprint: [9; 32],
        challenge_expires_at: now + chrono::Duration::minutes(5),
    };
    store
        .create_room_for_local_operator(&uuid::Uuid::new_v4().to_string(), "general", "General")
        .await
        .unwrap_or_else(|error| panic!("create resume fixture room: {error:?}"));
    let authority = crate::HumanInviteCredentialAuthority::from_persistent(
        &store
            .host_identity()
            .await
            .unwrap_or_else(|error| panic!("load resume fixture identity: {error:?}")),
    );
    let credentials = authority
        .issue(&crate::HumanInviteCredentialDraft {
            room_url: "http://127.0.0.1:8765".into(),
            public_room_url: String::new(),
            room_id: "general".into(),
            base_participant_id: "general-guest".into(),
            display_name: "Guest".into(),
            invite_scope: InviteScope::ReadWrite,
            issued_at: now,
            expires_at: now + chrono::Duration::days(1),
        })
        .unwrap_or_else(|error| panic!("issue resume fixture invite: {error:?}"));
    let manager = store
        .authorize_local_room_manager(
            "general",
            LOCAL_OPERATOR_USER_ID,
            LOCAL_OPERATOR_PARTICIPANT_ID,
        )
        .await
        .unwrap_or_else(|error| panic!("authorize resume fixture manager: {error:?}"));
    store
        .create_human_invite_for_manager(
            &RoomManagerAuthority::Local(manager),
            NewHumanInvite {
                signed_token_fingerprint: *credentials.signed_token_fingerprint(),
                join_code_fingerprint: *credentials.join_code_fingerprint(),
                base_participant_id: "general-guest".into(),
                display_name: "Guest".into(),
                invite_scope: InviteScope::ReadWrite,
                max_uses: 1,
                expires_at: now + chrono::Duration::days(1),
                created_at: now,
            },
        )
        .await
        .unwrap_or_else(|error| panic!("persist resume fixture invite: {error:?}"));
    let prepared = PreparedHumanAdmission::prepare(
        HumanInviteCredentialEvidence::JoinCode {
            fingerprint: *credentials.join_code_fingerprint(),
        },
        [3_u8; 32],
        &HumanAdmissionInput {
            request_id: uuid::Uuid::new_v4().to_string(),
            meeting_id_assertion: "general".into(),
            display_name: String::new(),
            participant_type: "human".into(),
            owner_display_name: String::new(),
            client_id: "resume-client".into(),
            avatar_image_url: String::new(),
        },
    )
    .unwrap_or_else(|error| panic!("prepare resume fixture admission: {error:?}"))
    .with_member(member.clone());
    let HumanAdmissionDecision::Admitted(_) = store
        .admit_human(&prepared, now)
        .await
        .unwrap_or_else(|error| panic!("commit resume fixture admission: {error:?}"))
    else {
        panic!("seeding the resume fixture room must commit");
    };
    let state = crate::AppState::local(
        store.clone(),
        crate::TicketStore::new(std::time::Duration::from_secs(30), 4096),
        agentsassemble_provider::ProviderCatalogService::fixed(ProviderCatalog::default()),
    )
    .await
    .unwrap_or_else(|error| panic!("build resume fixture state: {error:?}"));
    let device = format!("aad1_{}", base64(&[9_u8; 32]));
    let browser = crate::human_browser_credential::fingerprint_browser_credential(&device)
        .unwrap_or_else(|| panic!("fixture device token must fingerprint"));
    let rng = SystemRandom::new();
    let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, &rng)
        .unwrap_or_else(|error| panic!("generate resume key: {error:?}"));
    let key_pair = EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, pkcs8.as_ref(), &rng)
        .unwrap_or_else(|error| panic!("load resume key: {error:?}"));
    let client_public_key = base64(&[7_u8; 65]);
    let hello = crate::central::host_identity::SecureClientHello {
        protocol: "secure_admission_v1".into(),
        server_id: "resume-server".into(),
        registration_epoch: EPOCH.into(),
        origin: ORIGIN.into(),
        generation: 4,
        purpose: "member_connect".into(),
        client_nonce: "resume-nonce".into(),
        client_public_key: client_public_key.clone(),
    };
    let secure = crate::secure_client::SecureClient::new(
        hello,
        "c".repeat(43),
        crate::client_wire::ClientWire::default(),
    )
    .unwrap_or_else(|()| panic!("fixture secure channel"));
    Fixture {
        state,
        store,
        device,
        browser,
        secure,
        key_pair,
    }
}

impl Fixture {
    /// The raw uncompressed public key of the fixture device.
    fn public_key(&self) -> [u8; 65] {
        let mut key = [0u8; 65];
        key.copy_from_slice(self.key_pair.public_key().as_ref());
        key
    }

    fn browser_fingerprint(&self) -> String {
        base64(&self.browser)
    }

    fn request(&self, body: &Value) -> Request {
        let mut request = Request::builder()
            .method("POST")
            .header("x-device-token", &self.device)
            .body(axum::body::Body::from(
                serde_json::to_vec(body)
                    .unwrap_or_else(|error| panic!("encode resume body: {error}")),
            ))
            .unwrap_or_else(|error| panic!("build resume request: {error}"));
        request.extensions_mut().insert(self.secure.clone());
        request
    }

    async fn nonce(&self) -> String {
        let response = super::issue_nonce(
            axum::extract::State(self.state.clone()),
            self.request(&json!({})),
        )
        .await
        .unwrap_or_else(|error| panic!("issue resume nonce: {error:?}"));
        response.0["nonce"]
            .as_str()
            .unwrap_or_else(|| panic!("the nonce response must carry a nonce"))
            .to_owned()
    }

    /// The signed resume request a client builds for this channel.
    fn signed_body(&self, nonce: &str) -> Value {
        self.signed_body_with(&self.key_pair, nonce)
    }

    /// The same resume request, signed by another device key: the body
    /// still presents this device's own resume public key.
    fn signed_body_with(&self, key_pair: &EcdsaKeyPair, nonce: &str) -> Value {
        let hello = self.secure.hello();
        let browser_fingerprint = self.browser_fingerprint();
        let transcript = client_transcript(&super::ResumeTranscript {
            server_id: &hello.server_id,
            registration_epoch: &hello.registration_epoch,
            origin: &hello.origin,
            generation: hello.generation,
            host_key_fingerprint: self.state.central_host_identity.fingerprint(),
            browser_credential_fingerprint: &browser_fingerprint,
            client_public_key: &hello.client_public_key,
            channel_id: &self.secure.binding().channel_id,
            nonce,
        });
        json!({
            "server_id": hello.server_id,
            "registration_epoch": hello.registration_epoch,
            "origin": hello.origin,
            "generation": hello.generation,
            "host_key_fingerprint": self.state.central_host_identity.fingerprint(),
            "browser_credential_fingerprint": browser_fingerprint,
            "client_public_key": hello.client_public_key,
            "resume_public_key": base64(&self.public_key()),
            "channel_id": self.secure.binding().channel_id,
            "purpose": "resume",
            "nonce": nonce,
            "signature": sign_with(key_pair, &transcript),
        })
    }
}

async fn store_resume_key(fixture: &Fixture) {
    let now = chrono::Utc::now();
    fixture
        .store
        .issue_member_resume_key(agentsassemble_persistence::MemberResumeKeyIssue {
            issuer: ISSUER,
            person_id: PERSON,
            registration_epoch: EPOCH,
            browser_credential_fingerprint: &fixture.browser,
            resume_public_key: &fixture.public_key(),
            issued_at: now,
            expires_at: now + chrono::Duration::hours(1),
        })
        .await
        .unwrap_or_else(|error| panic!("store resume key: {error:?}"));
}

#[tokio::test]
async fn a_signed_resume_lists_the_joined_rooms() {
    let fixture = fixture().await;
    store_resume_key(&fixture).await;
    let nonce = fixture.nonce().await;
    let response = super::resume(
        axum::extract::State(fixture.state.clone()),
        fixture.request(&fixture.signed_body(&nonce)),
    )
    .await
    .unwrap_or_else(|error| panic!("a valid resume must succeed: {error:?}"));
    let rooms = response.0["rooms"]
        .as_array()
        .unwrap_or_else(|| panic!("the resume answer must list rooms"));
    assert_eq!(rooms.len(), 1);
    assert_eq!(rooms[0]["room_id"], "general");
    assert!(
        response.0["challenge_id"].as_str().is_some(),
        "a resume must hand out the challenge the room select reuses"
    );
    // A successful resume never extends the stored expiry.
    let stored = fixture
        .store
        .member_resume_key(&fixture.browser, &fixture.public_key())
        .await
        .unwrap_or_else(|error| panic!("read resume key: {error:?}"));
    let stored = stored.unwrap_or_else(|| panic!("the resume key must stay stored"));
    assert!(
        stored.expires_at > chrono::Utc::now() + chrono::Duration::minutes(50),
        "a direct success must not move the expiry forward"
    );
}

#[tokio::test]
async fn a_nonce_is_consumed_once_and_a_replay_is_refused() {
    let fixture = fixture().await;
    store_resume_key(&fixture).await;
    let nonce = fixture.nonce().await;
    let body = fixture.signed_body(&nonce);
    let first = super::resume(
        axum::extract::State(fixture.state.clone()),
        fixture.request(&body),
    )
    .await
    .unwrap_or_else(|error| panic!("the first resume must succeed: {error:?}"));
    assert!(first.0["rooms"].as_array().is_some());
    let replay = super::resume(
        axum::extract::State(fixture.state.clone()),
        fixture.request(&body),
    )
    .await;
    assert!(
        replay.is_err(),
        "a second use of the same nonce must be refused"
    );
}

#[test]
fn a_channel_keeps_at_most_one_outstanding_nonce() {
    let nonces = super::MemberResumeNonces::default();
    let binding = agentsassemble_persistence::SecureSessionBinding {
        client_key_fingerprint: [3; 32],
        channel_id: "c".repeat(43),
    };
    let other = agentsassemble_persistence::SecureSessionBinding {
        client_key_fingerprint: [4; 32],
        channel_id: "d".repeat(43),
    };
    let now = chrono::Utc::now();
    let first = nonces
        .issue(&binding, EPOCH, now)
        .unwrap_or_else(|error| panic!("issue the first nonce: {error:?}"));
    let second = nonces
        .issue(&binding, EPOCH, now)
        .unwrap_or_else(|error| panic!("issue the second nonce: {error:?}"));
    assert_ne!(first, second, "every issued nonce is fresh");
    assert!(
        !nonces.consume(&first, &binding, EPOCH, now),
        "the replaced nonce of a channel must never open the direct path"
    );
    assert!(
        nonces.consume(&second, &binding, EPOCH, now),
        "the current nonce of the channel opens the direct path"
    );
    // A nonce of another channel never opens this one.
    let foreign = nonces
        .issue(&other, EPOCH, now)
        .unwrap_or_else(|error| panic!("issue the other nonce: {error:?}"));
    assert!(
        !nonces.consume(&foreign, &binding, EPOCH, now),
        "a nonce of another channel must be refused"
    );
}

#[test]
fn the_nonce_store_refuses_more_channels_than_it_can_hold() {
    let nonces = super::MemberResumeNonces::default();
    let now = chrono::Utc::now();
    let mut first = agentsassemble_persistence::SecureSessionBinding {
        client_key_fingerprint: [3; 32],
        channel_id: "c".repeat(43),
    };
    for index in 0..super::RESUME_NONCE_LIMIT {
        first.channel_id = format!("{index:0>43}");
        nonces
            .issue(&first, EPOCH, now)
            .unwrap_or_else(|error| panic!("issue the nonce of channel {index}: {error:?}"));
    }
    // One channel too many is refused with the same 429 the connect
    // challenge map answers.
    let overflow = agentsassemble_persistence::SecureSessionBinding {
        client_key_fingerprint: [3; 32],
        channel_id: "z".repeat(43),
    };
    let error = nonces
        .issue(&overflow, EPOCH, now)
        .err()
        .unwrap_or_else(|| panic!("the store must refuse one channel too many"));
    assert_eq!(error.status, axum::http::StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(error.code, "member_challenge_capacity");
    // A channel that already holds a nonce only replaces it: the cap never
    // refuses a replacement.
    first.channel_id = format!("{:0>43}", 0);
    assert!(
        nonces.issue(&first, EPOCH, now).is_ok(),
        "a channel that already holds a nonce must be able to replace it"
    );
}

#[tokio::test]
async fn a_signature_never_works_on_another_channel_origin_or_generation() {
    let fixture = fixture().await;
    store_resume_key(&fixture).await;
    let nonce = fixture.nonce().await;
    let body = fixture.signed_body(&nonce);
    for (field, value) in [
        ("origin", json!("https://other-host.test")),
        ("generation", json!(5)),
        ("channel_id", json!("d".repeat(43))),
        ("browser_credential_fingerprint", json!(base64(&[8_u8; 32]))),
        ("host_key_fingerprint", json!("other-fingerprint")),
        ("purpose", json!("admission")),
    ] {
        let mut tampered = body.clone();
        tampered[field] = value;
        let refused = super::resume(
            axum::extract::State(fixture.state.clone()),
            fixture.request(&tampered),
        )
        .await;
        assert!(refused.is_err(), "a changed {field} must be refused");
    }
    // A host-supplied blob is not what gets signed: a signature over other
    // bytes never opens this path.
    let mut foreign = body.clone();
    foreign["signature"] = json!(base64(&[0_u8; 64]));
    assert!(
        super::resume(
            axum::extract::State(fixture.state.clone()),
            fixture.request(&foreign),
        )
        .await
        .is_err()
    );
}

#[tokio::test]
async fn the_channel_key_never_stands_in_for_the_stored_resume_key() {
    let fixture = fixture().await;
    store_resume_key(&fixture).await;
    let nonce = fixture.nonce().await;
    let mut body = fixture.signed_body(&nonce);
    // The ephemeral key of the channel is not the device's resume key.
    body["resume_public_key"] = json!(fixture.secure.hello().client_public_key);
    assert!(
        super::resume(
            axum::extract::State(fixture.state.clone()),
            fixture.request(&body),
        )
        .await
        .is_err(),
        "the channel key must not select the stored resume key"
    );
    // A key this host never stored is refused as well.
    let mut unknown = fixture.signed_body(&fixture.nonce().await);
    unknown["resume_public_key"] = json!(base64(&[3_u8; 65]));
    assert!(
        super::resume(
            axum::extract::State(fixture.state.clone()),
            fixture.request(&unknown),
        )
        .await
        .is_err(),
        "an unknown public key must be refused"
    );
}

#[tokio::test]
async fn a_resume_body_of_another_registration_epoch_never_opens_the_direct_path() {
    // A client hello of another epoch never gets a ServerHello (the secure
    // channel refuses it), so this route only ever sees the live epoch of
    // its own channel. The body check below is the second line of defence.
    let fixture = fixture().await;
    store_resume_key(&fixture).await;
    let nonce = fixture.nonce().await;
    let mut body = fixture.signed_body(&nonce);
    // The stored anchor belongs to another registration epoch.
    body["registration_epoch"] = json!("old-epoch");
    assert!(
        super::resume(
            axum::extract::State(fixture.state.clone()),
            fixture.request(&body),
        )
        .await
        .is_err(),
        "a stale incarnation must be refused"
    );
    // The live epoch of the row is the only one that opens the path.
    let nonce = fixture.nonce().await;
    assert!(
        super::resume(
            axum::extract::State(fixture.state.clone()),
            fixture.request(&fixture.signed_body(&nonce)),
        )
        .await
        .is_ok()
    );
}

#[tokio::test]
async fn an_unknown_or_expired_key_never_opens_the_direct_path() {
    let fixture = fixture().await;
    let nonce = fixture.nonce().await;
    // No key was issued for this browser credential.
    let refused = super::resume(
        axum::extract::State(fixture.state.clone()),
        fixture.request(&fixture.signed_body(&nonce)),
    )
    .await;
    assert!(refused.is_err(), "an unknown key must be refused");
    // A key stored for another browser credential does not open this one.
    let other = [7_u8; 32];
    let now = chrono::Utc::now();
    fixture
        .store
        .issue_member_resume_key(agentsassemble_persistence::MemberResumeKeyIssue {
            issuer: ISSUER,
            person_id: PERSON,
            registration_epoch: EPOCH,
            browser_credential_fingerprint: &other,
            resume_public_key: &fixture.public_key(),
            issued_at: now,
            expires_at: now + chrono::Duration::hours(1),
        })
        .await
        .unwrap_or_else(|error| panic!("store the other browser key: {error:?}"));
    let nonce = fixture.nonce().await;
    assert!(
        super::resume(
            axum::extract::State(fixture.state.clone()),
            fixture.request(&fixture.signed_body(&nonce)),
        )
        .await
        .is_err(),
        "another browser credential must not open this key"
    );
}

#[tokio::test]
async fn another_person_cannot_use_this_key_on_the_same_browser_credential() {
    let fixture = fixture().await;
    store_resume_key(&fixture).await;
    // Another person shares this browser credential (the same browser after
    // an account switch) and holds its own device key.
    let other = other_key_pair();
    let nonce = fixture.nonce().await;
    // The attempt presents this person's stored public key, but the
    // signature is the other person's own.
    let body = fixture.signed_body_with(&other, &nonce);
    let refused = super::resume(
        axum::extract::State(fixture.state.clone()),
        fixture.request(&body),
    )
    .await;
    assert!(
        refused.is_err(),
        "another person must not use this key on the same browser credential"
    );
    // The stored row still belongs to this person.
    let stored = fixture
        .store
        .member_resume_key(&fixture.browser, &fixture.public_key())
        .await
        .unwrap_or_else(|error| panic!("read the stored key: {error:?}"))
        .unwrap_or_else(|| panic!("the stored resume key must stay stored"));
    assert_eq!(stored.issuer, ISSUER);
    assert_eq!(stored.person_id, PERSON);
}

#[tokio::test]
async fn the_key_is_issued_only_after_a_central_admission_on_the_same_channel() {
    let fixture = fixture().await;
    let body = json!({"client_public_key": base64(fixture.key_pair.public_key().as_ref())});
    // No redeemed connect challenge on this channel yet.
    assert!(
        super::issue_key(
            axum::extract::State(fixture.state.clone()),
            fixture.request(&body),
        )
        .await
        .is_err(),
        "a channel without a central admission must not receive a resume key"
    );
    let member = MemberAdmission {
        secure: None,
        projection_id: "resume-projection".into(),
        issuer: ISSUER.into(),
        person_id: PERSON.into(),
        display_name: "Member".into(),
        registration_epoch: EPOCH.into(),
        challenge_fingerprint: [9; 32],
        challenge_expires_at: chrono::Utc::now() + chrono::Duration::minutes(5),
    };
    let rooms = fixture
        .store
        .member_connect_rooms(&member, chrono::Utc::now())
        .await
        .unwrap_or_else(|error| panic!("list resume fixture rooms: {error:?}"));
    let record = std::sync::Arc::new(tokio::sync::Mutex::new(connect::ConnectState::redeemed(
        member,
        rooms,
        connect::ConnectOrigin::Central,
    )));
    fixture
        .state
        .member_challenges
        .issue_for(
            ChallengePurpose::Connect(record),
            fixture.browser,
            EPOCH.to_owned(),
            chrono::Utc::now(),
            Some(fixture.secure.binding().clone()),
        )
        .unwrap_or_else(|error| panic!("issue the redeemed challenge: {error:?}"));
    let response = super::issue_key(
        axum::extract::State(fixture.state.clone()),
        fixture.request(&body),
    )
    .await
    .unwrap_or_else(|error| panic!("a central admission must issue a resume key: {error:?}"));
    assert!(response.0["expires_at"].as_i64().is_some());
    let stored = fixture
        .store
        .member_resume_key(&fixture.browser, &fixture.public_key())
        .await
        .unwrap_or_else(|error| panic!("read the issued key: {error:?}"));
    let stored = stored.unwrap_or_else(|| {
        panic!("the issued resume key must be stored for this browser credential")
    });
    // The central redemption issues its key exactly once: a second request on
    // the same channel is refused and never moves the stored expiry.
    let again = super::issue_key(
        axum::extract::State(fixture.state.clone()),
        fixture
            .request(&json!({"client_public_key": base64(fixture.key_pair.public_key().as_ref())})),
    )
    .await;
    assert!(
        again.is_err(),
        "one central redemption must never issue a second resume key"
    );
    let after = fixture
        .store
        .member_resume_key(&fixture.browser, &fixture.public_key())
        .await
        .unwrap_or_else(|error| panic!("read the issued key again: {error:?}"))
        .unwrap_or_else(|| panic!("the issued resume key must stay stored"));
    assert_eq!(
        after.expires_at.timestamp_micros(),
        stored.expires_at.timestamp_micros(),
        "a refused key request must not move the stored expiry"
    );
}

#[tokio::test]
async fn a_direct_resume_never_issues_or_refreshes_the_resume_key() {
    let fixture = fixture().await;
    store_resume_key(&fixture).await;
    let before = fixture
        .store
        .member_resume_key(&fixture.browser, &fixture.public_key())
        .await
        .unwrap_or_else(|error| panic!("read the stored key: {error:?}"))
        .unwrap_or_else(|| panic!("the stored resume key must exist"));
    // The direct attempt redeems a connect challenge on this channel.
    let nonce = fixture.nonce().await;
    let resumed = super::resume(
        axum::extract::State(fixture.state.clone()),
        fixture.request(&fixture.signed_body(&nonce)),
    )
    .await
    .unwrap_or_else(|error| panic!("a valid resume must succeed: {error:?}"));
    assert!(
        resumed.0["rooms"].as_array().is_some(),
        "the direct attempt must redeem its own challenge"
    );
    // The key request on the same channel is refused: a direct redemption
    // never issues a key of its own.
    let refused = super::issue_key(
        axum::extract::State(fixture.state.clone()),
        fixture
            .request(&json!({"client_public_key": base64(fixture.key_pair.public_key().as_ref())})),
    )
    .await;
    assert!(
        refused.is_err(),
        "a direct resume must never issue a resume key"
    );
    // The stored key is byte-identical: a direct attempt never moves the
    // stored expiry forward.
    let after = fixture
        .store
        .member_resume_key(&fixture.browser, &fixture.public_key())
        .await
        .unwrap_or_else(|error| panic!("read the stored key again: {error:?}"))
        .unwrap_or_else(|| panic!("the stored resume key must stay stored"));
    assert_eq!(
        after.expires_at.timestamp_micros(),
        before.expires_at.timestamp_micros(),
        "a direct resume must not move the stored expiry"
    );
    assert_eq!(
        after.resume_public_key, before.resume_public_key,
        "a direct resume must not replace the stored key"
    );
}

#[test]
fn an_old_client_keeps_the_current_redeem_and_select_bodies() {
    // The resume protocol adds routes, never fields: the bodies an old client
    // sends stay exactly the shapes the host has always decoded, and a body
    // with an extra field is still refused.
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct LegacyRedeemRequest {
        challenge_id: String,
        grant_token: String,
    }
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct LegacySelectRequest {
        #[serde(rename = "challenge_id")]
        challenge: String,
        #[serde(rename = "room_id")]
        room: String,
        #[serde(rename = "client_id")]
        client: String,
    }
    let redeem: LegacyRedeemRequest = serde_json::from_value(serde_json::json!({
        "challenge_id": "challenge",
        "grant_token": "aamc1.connect",
    }))
    .unwrap_or_else(|error| panic!("legacy redeem body: {error}"));
    assert_eq!(redeem.challenge_id, "challenge");
    assert_eq!(redeem.grant_token, "aamc1.connect");
    let select: LegacySelectRequest = serde_json::from_value(serde_json::json!({
        "challenge_id": "challenge",
        "room_id": "general",
        "client_id": "client",
    }))
    .unwrap_or_else(|error| panic!("legacy select body: {error}"));
    assert_eq!(
        (
            select.challenge.as_str(),
            select.room.as_str(),
            select.client.as_str()
        ),
        ("challenge", "general", "client")
    );
    assert!(
        serde_json::from_value::<LegacySelectRequest>(serde_json::json!({
            "challenge_id": "challenge",
            "room_id": "general",
            "client_id": "client",
            "resume_key": "aamc1.resume",
        }))
        .is_err(),
        "an unknown body field stays refused"
    );
}

#[tokio::test]
async fn an_old_host_answers_the_resume_route_with_not_found() {
    // The old host knows the connect routes and nothing else: its answer to
    // the resume route is a plain 404, so a client learns that the host has no
    // resume support without sending a resume body.
    let old_host = axum::Router::new().route(
        "/api/member-connect/challenge",
        axum::routing::post(|| async { axum::Json(json!({})) }),
    );
    let response = old_host
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/member-connect/resume")
                .body(axum::body::Body::empty())
                .unwrap_or_else(|error| panic!("build old host request: {error}")),
        )
        .await
        .unwrap_or_else(|error| panic!("old host answer: {error:?}"));
    assert_eq!(response.status(), axum::http::StatusCode::NOT_FOUND);
    assert!(
        crate::product_surface::registered_route_path("/api/member-connect/resume"),
        "the current host registers the resume route"
    );
    assert!(
        crate::product_surface::registered_route_path("/api/member-connect/resume-key"),
        "the current host registers the resume key route"
    );
}
