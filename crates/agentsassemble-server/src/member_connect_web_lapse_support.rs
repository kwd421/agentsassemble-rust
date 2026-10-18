//! Shared lapse-path fixture for the `member_connect_web` select tests.
//! Coverage rationale lives above `LAPSE_TEST_EPOCH` in the tests module.

pub(super) struct LapseFixture {
    pub(super) state: crate::AppState,
    pub(super) device: String,
    pub(super) browser: [u8; 32],
    pub(super) secure: crate::secure_client::SecureClient,
}

pub(super) async fn lapse_fixture(room_ids: &[(&str, &str)]) -> LapseFixture {
    use agentsassemble_domain::{
        InviteScope, LOCAL_OPERATOR_PARTICIPANT_ID, LOCAL_OPERATOR_USER_ID, ProviderCatalog,
    };
    use agentsassemble_persistence::{
        HumanAdmissionDecision, HumanAdmissionInput, HumanInviteCredentialEvidence,
        NewHumanInvite, PreparedHumanAdmission, RoomManagerAuthority, SqliteStore,
    };

    let store = SqliteStore::open(&format!(
        "sqlite:file:{}?mode=memory&cache=shared",
        uuid::Uuid::new_v4()
    ))
    .await
    .unwrap_or_else(|error| panic!("open lapse fixture store: {error:?}"));
    store
        .bootstrap_local_authority("325bb3a2-4964-42a2-8490-afcf6f99b164", "SeiNel")
        .await
        .unwrap_or_else(|error| panic!("bootstrap lapse fixture: {error:?}"));
    store
        .set_registration_epoch(Some(super::tests::LAPSE_TEST_EPOCH))
        .await
        .unwrap_or_else(|error| panic!("set lapse fixture epoch: {error:?}"));
    let device = format!(
        "aad1_{}",
        base64::Engine::encode(
            &base64::engine::general_purpose::URL_SAFE_NO_PAD,
            [9_u8; 32]
        )
    );
    let seed_browser = [3_u8; 32];
    let now = chrono::Utc::now();
    let seed_member = agentsassemble_persistence::MemberAdmission {
        secure: None,
        projection_id: "AAAAAAAAAAAAAAAAAAAAAA".into(),
        issuer: super::tests::LAPSE_TEST_ISSUER.into(),
        person_id: super::tests::LAPSE_TEST_PERSON.into(),
        display_name: "Member snapshot".into(),
        registration_epoch: super::tests::LAPSE_TEST_EPOCH.into(),
        challenge_fingerprint: [9; 32],
        challenge_expires_at: now + chrono::Duration::seconds(30),
    };
    let authority = crate::HumanInviteCredentialAuthority::from_persistent(
        &store
            .host_identity()
            .await
            .unwrap_or_else(|error| panic!("load lapse fixture identity: {error:?}")),
    );
    for (room_uid, room_id) in room_ids {
        store
            .create_room_for_local_operator(room_uid, room_id, room_id)
            .await
            .unwrap_or_else(|error| panic!("create lapse fixture room: {error:?}"));
        let credentials = authority
            .issue(&crate::HumanInviteCredentialDraft {
                room_url: "http://127.0.0.1:8765".into(),
                public_room_url: String::new(),
                room_id: (*room_id).to_owned(),
                base_participant_id: format!("{room_id}-guest"),
                display_name: format!("{room_id} Guest"),
                invite_scope: InviteScope::ReadWrite,
                issued_at: now,
                expires_at: now + chrono::Duration::days(1),
            })
            .unwrap_or_else(|error| panic!("issue lapse fixture invite: {error:?}"));
        let manager = store
            .authorize_local_room_manager(
                room_id,
                LOCAL_OPERATOR_USER_ID,
                LOCAL_OPERATOR_PARTICIPANT_ID,
            )
            .await
            .unwrap_or_else(|error| panic!("authorize lapse fixture manager: {error:?}"));
        store
            .create_human_invite_for_manager(
                &RoomManagerAuthority::Local(manager),
                NewHumanInvite {
                    signed_token_fingerprint: *credentials.signed_token_fingerprint(),
                    join_code_fingerprint: *credentials.join_code_fingerprint(),
                    base_participant_id: format!("{room_id}-guest"),
                    display_name: format!("{room_id} Guest"),
                    invite_scope: InviteScope::ReadWrite,
                    max_uses: 1,
                    expires_at: now + chrono::Duration::days(1),
                    created_at: now,
                },
            )
            .await
            .unwrap_or_else(|error| panic!("persist lapse fixture invite: {error:?}"));
        let prepared = PreparedHumanAdmission::prepare(
            HumanInviteCredentialEvidence::JoinCode {
                fingerprint: *credentials.join_code_fingerprint(),
            },
            seed_browser,
            &HumanAdmissionInput {
                request_id: uuid::Uuid::new_v4().to_string(),
                meeting_id_assertion: (*room_id).into(),
                display_name: String::new(),
                participant_type: "human".into(),
                owner_display_name: String::new(),
                client_id: "seed-second-client".into(),
                avatar_image_url: String::new(),
            },
        )
        .unwrap_or_else(|error| panic!("prepare lapse fixture admission: {error:?}"))
        .with_member(seed_member.clone());
        let HumanAdmissionDecision::Admitted(_) = store
            .admit_human(&prepared, now)
            .await
            .unwrap_or_else(|error| panic!("commit lapse fixture admission: {error:?}"))
        else {
            panic!("seeding the lapse fixture room must commit");
        };
    }
    let state = crate::AppState::local(
        store,
        crate::TicketStore::new(std::time::Duration::from_secs(30), 4096),
        agentsassemble_provider::ProviderCatalogService::fixed(ProviderCatalog::default()),
    )
    .await
    .unwrap_or_else(|error| panic!("build lapse fixture state: {error:?}"));
    let browser = crate::human_browser_credential::fingerprint_browser_credential(&device)
        .expect("fixture device token must fingerprint");
    let hello = crate::central::host_identity::SecureClientHello {
        protocol: "secure_admission_v1".into(),
        server_id: "channel-bound-test-server".into(),
        registration_epoch: super::tests::LAPSE_TEST_EPOCH.into(),
        origin: "https://channel-bound.test".into(),
        generation: 3,
        purpose: "member_connect".into(),
        client_nonce: "fixture-nonce".into(),
        client_public_key: base64::Engine::encode(
            &base64::engine::general_purpose::URL_SAFE_NO_PAD,
            [7_u8; 32],
        ),
    };
    let secure = crate::secure_client::SecureClient::new(hello, "a".repeat(43), false)
        .unwrap_or_else(|()| panic!("fixture secure channel"));
    LapseFixture {
        state,
        device,
        browser,
        secure,
    }
}
