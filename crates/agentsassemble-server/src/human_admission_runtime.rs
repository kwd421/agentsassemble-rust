use agentsassemble_domain::RoomEvent;
use agentsassemble_persistence::{
    HumanAdmissionDecision, PersistenceError, PreparedHumanAdmission, SqliteStore,
};
use tokio::sync::{broadcast, oneshot};

use crate::event_publication::publish_durable_room_events;

pub(crate) struct HumanAdmissionCommand {
    pub(crate) request: PreparedHumanAdmission,
    pub(crate) reply: oneshot::Sender<Result<HumanAdmissionDecision, PersistenceError>>,
}

pub(crate) async fn handle_human_admission(
    store: &SqliteStore,
    room_id: &str,
    events: &broadcast::Sender<RoomEvent>,
    session_revocations: &broadcast::Sender<[u8; 32]>,
    command: HumanAdmissionCommand,
) {
    let decision = store
        .admit_human(&command.request, chrono::Utc::now())
        .await;
    if let Ok(HumanAdmissionDecision::Admitted(commit)) = &decision {
        for fingerprint in commit.replaced_session_fingerprints() {
            let _ = session_revocations.send(*fingerprint);
        }
        if commit.events().iter().any(|event| event.room_id == room_id) {
            publish_durable_room_events(store, events, room_id).await;
        }
    }
    let _ = command.reply.send(decision);
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use agentsassemble_domain::{
        InviteScope, LOCAL_OPERATOR_PARTICIPANT_ID, LOCAL_OPERATOR_USER_ID, ProviderCatalog,
    };
    use agentsassemble_persistence::{
        HumanAdmissionCommit, HumanAdmissionDecision, HumanAdmissionInput,
        HumanInviteCredentialEvidence, NewHumanInvite, PreparedHumanAdmission, SqliteStore,
    };
    use agentsassemble_provider::ProviderCatalogService;
    use chrono::{DateTime, Utc};
    use tokio::{sync::broadcast, time::timeout};

    use super::{HumanAdmissionCommand, handle_human_admission};
    use crate::{
        HumanInviteCredentialAuthority, HumanInviteCredentialDraft, RoomRuntime,
        human_session_bearer::fingerprint_presented_bearer,
    };

    #[tokio::test]
    async fn room_owner_publishes_admission_and_notifies_replaced_session() {
        let (store, authority, now) = fixture().await;
        let rooms = RoomRuntime::new(
            store.clone(),
            ProviderCatalogService::fixed(ProviderCatalog::default()),
        );
        let mut events = rooms.subscribe("general").await;
        let mut revocations = rooms.session_revocations("general").await;
        let first = admitted(
            rooms
                .admit_human(invitation(&store, &authority, now, "general", 1, "First").await)
                .await
                .unwrap_or_else(|error| panic!("admit first human: {error}")),
        );
        let first_fingerprint = fingerprint_presented_bearer(first.session_bearer())
            .unwrap_or_else(|| panic!("issued bearer must be canonical"));
        let joined = timeout(Duration::from_secs(1), events.recv())
            .await
            .unwrap_or_else(|_| panic!("participant joined publication timed out"))
            .unwrap_or_else(|error| panic!("receive participant joined: {error}"));
        assert_eq!(joined.event_type, "participant_joined");

        let second = admitted(
            rooms
                .admit_human(invitation(&store, &authority, now, "general", 2, "Second").await)
                .await
                .unwrap_or_else(|error| panic!("admit replacement human: {error}")),
        );
        assert_eq!(second.replaced_session_fingerprints(), &[first_fingerprint]);
        assert_eq!(
            timeout(Duration::from_secs(1), revocations.recv())
                .await
                .unwrap_or_else(|_| panic!("session replacement notification timed out"))
                .unwrap_or_else(|error| panic!("receive session replacement: {error}")),
            first_fingerprint
        );
        let updated = timeout(Duration::from_secs(1), events.recv())
            .await
            .unwrap_or_else(|_| panic!("participant update publication timed out"))
            .unwrap_or_else(|error| panic!("receive participant update: {error}"));
        assert_eq!(updated.event_type, "participant_updated");
        rooms
            .shutdown()
            .await
            .unwrap_or_else(|error| panic!("shutdown rooms: {error}"));
    }

    #[tokio::test]
    async fn cross_room_profile_events_stay_on_their_exact_room_streams() {
        let (store, authority, now) = fixture().await;
        create_room(
            &store,
            "alpha",
            "Alpha",
            "58585858-5858-4858-8858-585858585858",
        )
        .await;
        create_room(
            &store,
            "zeta",
            "Zeta",
            "59595959-5959-4959-8959-595959595959",
        )
        .await;
        let rooms = RoomRuntime::new(
            store.clone(),
            ProviderCatalogService::fixed(ProviderCatalog::default()),
        );
        let mut alpha_events = rooms.subscribe("alpha").await;
        let mut zeta_events = rooms.subscribe("zeta").await;
        admitted(
            rooms
                .admit_human(invitation(&store, &authority, now, "alpha", 4, "First").await)
                .await
                .unwrap_or_else(|error| panic!("admit alpha human: {error}")),
        );
        let alpha_joined = timeout(Duration::from_secs(1), alpha_events.recv())
            .await
            .unwrap_or_else(|_| panic!("alpha join publication timed out"))
            .unwrap_or_else(|error| panic!("receive alpha join: {error}"));
        assert_eq!(
            (
                alpha_joined.room_id.as_str(),
                alpha_joined.event_type.as_str()
            ),
            ("alpha", "participant_joined")
        );

        admitted(
            rooms
                .admit_human(invitation(&store, &authority, now, "zeta", 5, "Second").await)
                .await
                .unwrap_or_else(|error| panic!("admit zeta human: {error}")),
        );
        let zeta_joined = timeout(Duration::from_secs(1), zeta_events.recv())
            .await
            .unwrap_or_else(|_| panic!("zeta join publication timed out"))
            .unwrap_or_else(|error| panic!("receive zeta join: {error}"));
        assert_eq!(
            (
                zeta_joined.room_id.as_str(),
                zeta_joined.event_type.as_str()
            ),
            ("zeta", "participant_joined")
        );
        let alpha_updated = timeout(Duration::from_secs(1), alpha_events.recv())
            .await
            .unwrap_or_else(|_| panic!("alpha update publication timed out"))
            .unwrap_or_else(|error| panic!("receive alpha update: {error}"));
        assert_eq!(
            (
                alpha_updated.room_id.as_str(),
                alpha_updated.event_type.as_str()
            ),
            ("alpha", "participant_updated")
        );
        rooms
            .shutdown()
            .await
            .unwrap_or_else(|error| panic!("shutdown rooms: {error}"));
    }

    #[tokio::test]
    async fn dropped_http_reply_does_not_cancel_dequeued_admission() {
        let (store, authority, now) = fixture().await;
        let request = invitation(&store, &authority, now, "general", 3, "Cancelled Reply").await;
        let (event_tx, mut events) = broadcast::channel(4);
        let (revocation_tx, _) = broadcast::channel(4);
        let (reply, response) = tokio::sync::oneshot::channel();
        drop(response);

        handle_human_admission(
            &store,
            "general",
            &event_tx,
            &revocation_tx,
            HumanAdmissionCommand { request, reply },
        )
        .await;
        let event = timeout(Duration::from_secs(1), events.recv())
            .await
            .unwrap_or_else(|_| panic!("committed admission publication timed out"))
            .unwrap_or_else(|error| panic!("receive committed admission: {error}"));
        assert_eq!(event.event_type, "participant_joined");
    }

    async fn fixture() -> (SqliteStore, HumanInviteCredentialAuthority, DateTime<Utc>) {
        let store = SqliteStore::open("sqlite::memory:")
            .await
            .unwrap_or_else(|error| panic!("open store: {error}"));
        store
            .bootstrap_local_authority("56565656-5656-4656-8656-565656565656", "Host")
            .await
            .unwrap_or_else(|error| panic!("bootstrap authority: {error}"));
        store
            .create_room_for_local_operator(
                "57575757-5757-4757-8757-575757575757",
                "general",
                "General",
            )
            .await
            .unwrap_or_else(|error| panic!("create room: {error}"));
        store
            .acknowledge_room_publication("general", 1)
            .await
            .unwrap_or_else(|error| panic!("acknowledge room baseline: {error}"));
        let identity = store
            .host_identity()
            .await
            .unwrap_or_else(|error| panic!("load host identity: {error}"));
        (
            store,
            HumanInviteCredentialAuthority::from_persistent(&identity),
            Utc::now(),
        )
    }

    async fn invitation(
        store: &SqliteStore,
        authority: &HumanInviteCredentialAuthority,
        now: DateTime<Utc>,
        room_id: &str,
        suffix: u8,
        display_name: &str,
    ) -> PreparedHumanAdmission {
        let draft = HumanInviteCredentialDraft {
            room_url: "http://127.0.0.1:8765".to_owned(),
            public_room_url: String::new(),
            room_id: room_id.to_owned(),
            base_participant_id: format!("invite-guest-{suffix}"),
            display_name: format!("Invite Guest {suffix}"),
            invite_scope: InviteScope::ReadWrite,
            issued_at: now,
            expires_at: now + chrono::Duration::days(1),
        };
        let issued = authority
            .issue(&draft)
            .unwrap_or_else(|error| panic!("issue invite: {error}"));
        let manager = store
            .authorize_local_room_manager(
                room_id,
                LOCAL_OPERATOR_USER_ID,
                LOCAL_OPERATOR_PARTICIPANT_ID,
            )
            .await
            .unwrap_or_else(|error| panic!("authorize manager: {error}"));
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
                    expires_at: draft.expires_at,
                    created_at: draft.issued_at,
                },
            )
            .await
            .unwrap_or_else(|error| panic!("persist invite: {error}"));
        PreparedHumanAdmission::prepare(
            HumanInviteCredentialEvidence::JoinCode {
                fingerprint: *issued.join_code_fingerprint(),
            },
            [0x33; 32],
            &HumanAdmissionInput {
                request_id: format!("{suffix:08x}-1111-4111-8111-111111111111"),
                meeting_id_assertion: String::new(),
                display_name: display_name.to_owned(),
                participant_type: "human".to_owned(),
                owner_display_name: "Host".to_owned(),
                client_id: "browser-client".to_owned(),
                avatar_image_url: String::new(),
            },
        )
        .unwrap_or_else(|error| panic!("prepare admission: {error}"))
    }

    async fn create_room(store: &SqliteStore, room_id: &str, label: &str, request_id: &str) {
        store
            .create_room_for_local_operator(request_id, room_id, label)
            .await
            .unwrap_or_else(|error| panic!("create {room_id}: {error}"));
        store
            .acknowledge_room_publication(room_id, 1)
            .await
            .unwrap_or_else(|error| panic!("acknowledge {room_id} baseline: {error}"));
    }

    fn admitted(decision: HumanAdmissionDecision) -> Box<HumanAdmissionCommit> {
        match decision {
            HumanAdmissionDecision::Admitted(commit) => commit,
            HumanAdmissionDecision::Rejected(_) => panic!("expected admitted decision"),
        }
    }
}
