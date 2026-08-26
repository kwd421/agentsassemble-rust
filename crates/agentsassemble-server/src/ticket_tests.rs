use std::time::Duration;

use agentsassemble_domain::{
    AuthenticatedPrincipal, CapabilitySet, ClientKind, InviteScope, LOCAL_OPERATOR_PARTICIPANT_ID,
    LOCAL_OPERATOR_USER_ID,
};
use agentsassemble_persistence::{
    HumanAdmissionDecision, HumanAdmissionInput, HumanInviteCredentialEvidence,
    HumanSessionAuthorization, NewHumanInvite, PreparedHumanAdmission, SqliteStore,
};
use chrono::{Duration as ChronoDuration, Utc};

use crate::{
    TicketError, TicketStore,
    human_session_bearer::fingerprint_presented_bearer,
    ticket::{ConsumedSocketTicket, SocketTicketHint},
};

fn principal() -> AuthenticatedPrincipal {
    AuthenticatedPrincipal {
        principal_id: "operator".to_owned(),
        participant_id: "operator-local".to_owned(),
        display_name: "Host".to_owned(),
        room_id: "general".to_owned(),
        client_kind: ClientKind::Browser,
        invite_scope: InviteScope::ReadWrite,
        is_operator: true,
        capabilities: CapabilitySet::local_operator(ClientKind::Browser, InviteScope::ReadWrite),
    }
}

#[tokio::test]
async fn ticket_is_consumed_once() {
    let store = TicketStore::new(Duration::from_secs(30), 8);
    let ticket = store
        .issue(principal())
        .await
        .unwrap_or_else(|error| panic!("issue ticket: {error}"));
    assert!(store.consume(&ticket.ticket).await.is_ok());
    assert_eq!(
        store.consume(&ticket.ticket).await,
        Err(TicketError::Invalid)
    );
}

#[tokio::test]
async fn expired_ticket_fails_closed() {
    let store = TicketStore::new(Duration::ZERO, 8);
    let ticket = store
        .issue(principal())
        .await
        .unwrap_or_else(|error| panic!("issue ticket: {error}"));
    assert_eq!(
        store.consume(&ticket.ticket).await,
        Err(TicketError::Invalid)
    );
}

#[tokio::test]
async fn ticket_purposes_are_one_use_and_never_interchangeable() {
    let store = TicketStore::new(Duration::from_secs(30), 8);
    let operator = store
        .issue_server_operator("operator-local-user".to_owned())
        .await
        .unwrap_or_else(|error| panic!("issue operator ticket: {error}"));
    assert_eq!(
        store.consume(&operator.ticket).await,
        Err(TicketError::Invalid)
    );
    assert_eq!(
        store.consume_server_operator(&operator.ticket).await,
        Err(TicketError::Invalid)
    );

    let room = store
        .issue(principal())
        .await
        .unwrap_or_else(|error| panic!("issue room ticket: {error}"));
    assert_eq!(
        store.consume_server_operator(&room.ticket).await,
        Err(TicketError::Invalid)
    );
    assert_eq!(store.consume(&room.ticket).await, Err(TicketError::Invalid));
}

#[tokio::test]
async fn central_registration_ticket_is_not_generic_operator_or_profile_authority() {
    let store = TicketStore::new(Duration::from_secs(30), 8);
    let registration = store
        .issue_central_registration("operator-local-user".to_owned())
        .await
        .unwrap_or_else(|error| panic!("issue registration ticket: {error}"));
    assert_eq!(
        store.consume_server_operator(&registration.ticket).await,
        Err(TicketError::Invalid)
    );
    assert_eq!(
        store
            .consume_central_registration(&registration.ticket)
            .await,
        Err(TicketError::Invalid)
    );

    let profile_rejected = store
        .issue_central_registration("operator-local-user".to_owned())
        .await
        .unwrap_or_else(|error| panic!("issue profile-rejected ticket: {error}"));
    assert!(matches!(
        store.consume_profile(&profile_rejected.ticket).await,
        Err(TicketError::Invalid)
    ));
    assert_eq!(
        store
            .consume_central_registration(&profile_rejected.ticket)
            .await,
        Err(TicketError::Invalid)
    );
}

#[tokio::test]
async fn room_http_purposes_and_asset_bindings_are_consumed_on_mismatch() {
    let store = TicketStore::new(Duration::from_secs(30), 8);
    let preference = store
        .issue_preferences_read(
            "general".to_owned(),
            "operator-local-user".to_owned(),
            "operator-local".to_owned(),
        )
        .await
        .unwrap_or_else(|error| panic!("issue preference read: {error}"));
    assert_eq!(
        store.consume_preferences_write(&preference.ticket).await,
        Err(TicketError::Invalid)
    );
    assert_eq!(
        store.consume_preferences_read(&preference.ticket).await,
        Err(TicketError::Invalid)
    );

    let asset = store
        .issue_pending_preview_read(
            "general".to_owned(),
            "operator-local-user".to_owned(),
            "operator-local".to_owned(),
            "ra_00000000000000000000000000000000".to_owned(),
        )
        .await
        .unwrap_or_else(|error| panic!("issue pending preview read: {error}"));
    assert_eq!(
        store
            .consume_pending_preview_read(&asset.ticket, "ra_11111111111111111111111111111111",)
            .await,
        Err(TicketError::Invalid)
    );
    assert_eq!(
        store
            .consume_pending_preview_read(&asset.ticket, "ra_00000000000000000000000000000000",)
            .await,
        Err(TicketError::Invalid)
    );
}

#[tokio::test]
async fn settings_directory_ticket_never_crosses_room_or_profile_scopes() {
    let store = TicketStore::new(Duration::from_secs(30), 8);
    let directory = store
        .issue_settings_directory_read("operator-local-user".to_owned())
        .await
        .unwrap_or_else(|error| panic!("issue directory read: {error}"));
    assert!(matches!(
        store.consume_profile(&directory.ticket).await,
        Err(TicketError::Invalid)
    ));
    assert_eq!(
        store
            .consume_settings_directory_read(&directory.ticket)
            .await,
        Err(TicketError::Invalid)
    );
}

#[tokio::test]
async fn human_session_grants_are_exact_purpose_and_one_use() {
    let fixture = HumanSessionFixture::new(1).await;
    let store = TicketStore::new(Duration::from_secs(30), 4_096);
    assert!(matches!(
        store
            .issue_human_session_preferences_write(fixture.authorize(0).await)
            .await,
        Err(TicketError::Invalid)
    ));
    let preferences = store
        .issue_human_session_preferences_read(fixture.authorize(0).await)
        .await
        .unwrap_or_else(|error| panic!("issue read-only preference grant: {error}"));
    assert!(
        store
            .consume_human_session_preferences_read(&preferences.ticket)
            .await
            .is_ok()
    );
    let profile = store
        .issue_human_session_profile(fixture.authorize(0).await)
        .await
        .unwrap_or_else(|error| panic!("issue session profile grant: {error}"));
    assert!(matches!(
        store
            .consume_human_session_preferences_read(&profile.ticket)
            .await,
        Err(TicketError::Invalid)
    ));
    assert!(matches!(
        store.consume_human_session_profile(&profile.ticket).await,
        Err(TicketError::Invalid)
    ));

    let socket = store
        .issue_human_session_socket(fixture.authorize(0).await)
        .await
        .unwrap_or_else(|error| panic!("issue session socket grant: {error}"));
    assert!(matches!(
        store.socket_ticket_hint(&socket.ticket).await,
        Ok(SocketTicketHint::HumanSession { room_id }) if room_id == "general"
    ));
    let consumed = store
        .consume_socket(&socket.ticket)
        .await
        .unwrap_or_else(|error| panic!("consume session socket grant: {error}"));
    let ConsumedSocketTicket::HumanSession(consumed) = consumed else {
        panic!("human socket grant resolved as local authority");
    };
    let (authorization, proof_key, connection_nonce) = consumed.into_parts();
    assert_eq!(
        authorization.session_fingerprint(),
        &fixture.fingerprints[0]
    );
    assert_eq!(proof_key, socket.proof_key);
    assert_eq!(connection_nonce.len(), 64);
    assert!(matches!(
        store.consume_human_session_socket(&socket.ticket).await,
        Err(TicketError::Invalid)
    ));
}

#[tokio::test]
async fn socket_hint_consumes_wrong_purpose_without_cross_authority_fallback() {
    let fixture = HumanSessionFixture::new(1).await;
    let store = TicketStore::new(Duration::from_secs(30), 4_096);
    let profile = store
        .issue_human_session_profile(fixture.authorize(0).await)
        .await
        .unwrap_or_else(|error| panic!("issue wrong-purpose profile grant: {error}"));

    assert!(matches!(
        store.socket_ticket_hint(&profile.ticket).await,
        Err(TicketError::Invalid)
    ));
    assert!(matches!(
        store.consume_human_session_profile(&profile.ticket).await,
        Err(TicketError::Invalid)
    ));
}

#[tokio::test]
async fn human_session_grant_rechecks_absolute_expiry_after_issue() {
    let fixture = HumanSessionFixture::new(1).await;
    let store = TicketStore::new(Duration::from_secs(30), 4_096);
    let authorization = fixture.authorize(0).await;
    let after_session_expiry = authorization.expires_at() + ChronoDuration::microseconds(1);
    let profile = store
        .issue_human_session_profile(authorization)
        .await
        .unwrap_or_else(|error| panic!("issue session profile grant: {error}"));

    assert!(matches!(
        store
            .consume_human_session_profile_at(&profile.ticket, after_session_expiry)
            .await,
        Err(TicketError::Invalid)
    ));
    assert!(matches!(
        store.consume_human_session_profile(&profile.ticket).await,
        Err(TicketError::Invalid)
    ));
}

#[tokio::test]
async fn human_session_grants_enforce_per_session_limit_and_reclaim_consumption() {
    let fixture = HumanSessionFixture::new(1).await;
    let store = TicketStore::new(Duration::from_secs(30), 4_096);
    let mut issued = Vec::new();
    for _ in 0..8 {
        issued.push(
            store
                .issue_human_session_profile(fixture.authorize(0).await)
                .await
                .unwrap_or_else(|error| panic!("issue bounded session grant: {error}")),
        );
    }
    assert!(matches!(
        store
            .issue_human_session_profile(fixture.authorize(0).await)
            .await,
        Err(TicketError::Invalid)
    ));
    store
        .consume_human_session_profile(&issued[0].ticket)
        .await
        .unwrap_or_else(|error| panic!("consume bounded session grant: {error}"));
    assert!(
        store
            .issue_human_session_profile(fixture.authorize(0).await)
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn public_partition_preserves_private_reserve_and_reclaims_both_classes() {
    let fixture = HumanSessionFixture::new(3).await;
    let store = TicketStore::new(Duration::from_secs(30), 2_320);
    let mut public = Vec::new();
    for session in 0..2 {
        for _ in 0..8 {
            public.push(
                store
                    .issue_human_session_profile(fixture.authorize(session).await)
                    .await
                    .unwrap_or_else(|error| panic!("fill public partition: {error}")),
            );
        }
    }
    assert!(matches!(
        store
            .issue_human_session_profile(fixture.authorize(2).await)
            .await,
        Err(TicketError::Invalid)
    ));

    let mut private = Vec::new();
    for _ in 0..2_304 {
        private.push(
            store
                .issue(principal())
                .await
                .unwrap_or_else(|error| panic!("fill private reserve: {error}")),
        );
    }
    assert_eq!(store.issue(principal()).await, Err(TicketError::Invalid));

    store
        .consume_human_session_profile(&public[0].ticket)
        .await
        .unwrap_or_else(|error| panic!("reclaim public grant: {error}"));
    assert!(
        store
            .issue_human_session_profile(fixture.authorize(2).await)
            .await
            .is_ok()
    );
    assert_eq!(store.issue(principal()).await, Err(TicketError::Invalid));
    store
        .consume(&private[0].ticket)
        .await
        .unwrap_or_else(|error| panic!("reclaim private grant: {error}"));
    assert!(store.issue(principal()).await.is_ok());

    let production = TicketStore::new(Duration::from_secs(30), 4_096);
    assert_eq!(production.public_session_capacity(), 1_792);
}

pub(crate) struct HumanSessionFixture {
    store: SqliteStore,
    fingerprints: Vec<[u8; 32]>,
}

impl HumanSessionFixture {
    pub(crate) async fn new(count: usize) -> Self {
        let store = SqliteStore::open("sqlite::memory:")
            .await
            .unwrap_or_else(|error| panic!("open human session fixture: {error}"));
        store
            .bootstrap_local_authority("62e24662-d666-4166-bb60-c131412585c5", "Host")
            .await
            .unwrap_or_else(|error| panic!("bootstrap human session fixture: {error}"));
        store
            .create_room_for_local_operator(
                "80efcfd2-75e0-40b7-9e2a-d735485ef7e8",
                "general",
                "General",
            )
            .await
            .unwrap_or_else(|error| panic!("create human session room: {error}"));
        let manager = store
            .authorize_local_room_manager(
                "general",
                LOCAL_OPERATOR_USER_ID,
                LOCAL_OPERATOR_PARTICIPANT_ID,
            )
            .await
            .unwrap_or_else(|error| panic!("authorize human session manager: {error}"));
        let now = Utc::now();
        let mut fingerprints = Vec::with_capacity(count);
        for index in 0..count {
            let marker = u8::try_from(index + 1)
                .unwrap_or_else(|_| panic!("human session fixture marker overflow"));
            let join_fingerprint = [marker.saturating_add(0x40); 32];
            store
                .create_human_invite_for_local_manager(
                    &manager,
                    NewHumanInvite {
                        signed_token_fingerprint: [marker; 32],
                        join_code_fingerprint: join_fingerprint,
                        base_participant_id: format!("ticket-guest-{index}"),
                        display_name: format!("Ticket Guest {index}"),
                        invite_scope: if index == 0 {
                            InviteScope::ReadOnly
                        } else {
                            InviteScope::ReadWrite
                        },
                        max_uses: 1,
                        expires_at: now + ChronoDuration::hours(2),
                        created_at: now,
                    },
                )
                .await
                .unwrap_or_else(|error| panic!("create human session invite: {error}"));
            let prepared = PreparedHumanAdmission::prepare(
                HumanInviteCredentialEvidence::JoinCode {
                    fingerprint: join_fingerprint,
                },
                [marker.saturating_add(0x60); 32],
                &HumanAdmissionInput {
                    request_id: format!("00000000-0000-4000-8000-{:012x}", index + 1),
                    meeting_id_assertion: "general".to_owned(),
                    display_name: format!("Ticket Guest {index}"),
                    participant_type: "human".to_owned(),
                    owner_display_name: "Host".to_owned(),
                    client_id: format!("ticket-browser-{index}"),
                    avatar_image_url: String::new(),
                },
            )
            .unwrap_or_else(|error| panic!("prepare human session admission: {error}"));
            let commit = match store
                .admit_human(&prepared, now)
                .await
                .unwrap_or_else(|error| panic!("admit human session fixture: {error}"))
            {
                HumanAdmissionDecision::Admitted(commit) => commit,
                HumanAdmissionDecision::Rejected(rejection) => {
                    panic!("human session fixture rejected: {rejection:?}")
                }
            };
            fingerprints.push(
                fingerprint_presented_bearer(commit.session_bearer())
                    .unwrap_or_else(|| panic!("admission returned invalid session bearer")),
            );
        }
        Self {
            store,
            fingerprints,
        }
    }

    pub(crate) const fn store(&self) -> &SqliteStore {
        &self.store
    }

    pub(crate) async fn authorize(&self, index: usize) -> HumanSessionAuthorization {
        self.store
            .authorize_human_session(&self.fingerprints[index])
            .await
            .unwrap_or_else(|error| panic!("authorize ticket session: {error}"))
    }
}
