use agentsassemble_persistence::{PersistenceError, SqliteStore};
use tokio::sync::broadcast;

pub(crate) async fn drain_room_publications(
    store: &SqliteStore,
    events: &broadcast::Sender<agentsassemble_domain::RoomEvent>,
    room_id: &str,
) -> Result<(), PersistenceError> {
    loop {
        let pending = store.pending_room_publications(room_id).await?;
        if pending.is_empty() {
            return Ok(());
        }
        for event in pending {
            let event_seq = event.seq;
            let _ = events.send(event);
            store
                .acknowledge_room_publication(room_id, event_seq)
                .await?;
        }
    }
}

#[cfg(test)]
mod tests {
    use agentsassemble_domain::{
        AuthenticatedPrincipal, CapabilitySet, ClientKind, InviteScope, ProviderCatalog,
        UserProfilePatch,
    };
    use agentsassemble_persistence::SqliteStore;
    use agentsassemble_provider::ProviderCatalogService;

    use crate::RoomRuntime;

    #[tokio::test]
    async fn room_owner_publishes_external_profile_commit_from_durable_history() {
        let (store, principal) = fixture().await;
        let rooms = RoomRuntime::new(
            store.clone(),
            ProviderCatalogService::fixed(ProviderCatalog::default()),
        );
        let mut receiver = rooms.subscribe("general").await;
        let outcome = store
            .update_user_profile(
                &principal,
                UserProfilePatch {
                    display_name: Some("Publication Owner".to_owned()),
                    ..UserProfilePatch::default()
                },
            )
            .await
            .unwrap_or_else(|error| panic!("commit external profile update: {error}"));
        rooms.notify_committed_events(&outcome.events).await;
        let event = receiver
            .recv()
            .await
            .unwrap_or_else(|error| panic!("receive durable profile event: {error}"));
        assert_eq!(event.seq, 2);
        assert_eq!(event.display_name.as_deref(), Some("Publication Owner"));
        assert!(
            store
                .pending_room_publications("general")
                .await
                .unwrap_or_else(|error| panic!("read publication cursor: {error}"))
                .is_empty()
        );
        rooms
            .shutdown()
            .await
            .unwrap_or_else(|error| panic!("shutdown room owner: {error}"));
    }

    #[tokio::test]
    async fn blocked_profile_publication_cannot_be_overtaken_by_a_later_room_command() {
        let (store, principal) = fixture().await;
        let profile = store
            .update_user_profile(
                &principal,
                UserProfilePatch {
                    display_name: Some("Committed First".to_owned()),
                    ..UserProfilePatch::default()
                },
            )
            .await
            .unwrap_or_else(|error| panic!("commit blocked profile event: {error}"));
        assert_eq!(profile.events[0].seq, 2);
        let command = store
            .execute_message(
                &principal,
                "later-room-command",
                "message.send",
                &serde_json::json!({"content": "committed second"}),
            )
            .await
            .unwrap_or_else(|error| panic!("commit later room event: {error}"));
        assert_eq!(command.event.seq, 3);

        let (sender, mut receiver) = tokio::sync::broadcast::channel(4);
        super::drain_room_publications(&store, &sender, "general")
            .await
            .unwrap_or_else(|error| panic!("drain blocked publications: {error}"));
        let first = receiver
            .recv()
            .await
            .unwrap_or_else(|error| panic!("receive first publication: {error}"));
        let second = receiver
            .recv()
            .await
            .unwrap_or_else(|error| panic!("receive second publication: {error}"));
        assert_eq!((first.seq, second.seq), (2, 3));
        assert_eq!(first.event_type, "participant_updated");
        assert_eq!(second.event_type, "message_final");
        let snapshot = store
            .snapshot("general", 0, 200)
            .await
            .unwrap_or_else(|error| panic!("read publication snapshot: {error}"));
        assert_eq!(snapshot.last_seq, second.seq);
    }

    #[tokio::test]
    async fn recovered_turn_handoff_completes_only_after_durable_publication() {
        let (store, principal) = fixture().await;
        let rooms = RoomRuntime::new(
            store.clone(),
            ProviderCatalogService::fixed(ProviderCatalog::default()),
        );
        let mut receiver = rooms.subscribe("general").await;
        let command = store
            .execute_message(
                &principal,
                "recovery-publication-order",
                "message.send",
                &serde_json::json!({"content": "publish before recovered provider entry"}),
            )
            .await
            .unwrap_or_else(|error| panic!("commit recovery publication: {error}"));

        rooms
            .publish_then_resume_assigned_turns("general", Vec::new())
            .await
            .unwrap_or_else(|error| panic!("complete ordered recovery handoff: {error}"));
        let published = receiver
            .recv()
            .await
            .unwrap_or_else(|error| panic!("receive recovery publication: {error}"));
        assert_eq!(published.seq, command.event.seq);
        assert!(
            store
                .pending_room_publications("general")
                .await
                .unwrap_or_else(|error| panic!("read recovery publication cursor: {error}"))
                .is_empty()
        );
        rooms
            .shutdown()
            .await
            .unwrap_or_else(|error| panic!("shutdown ordered recovery room: {error}"));
    }

    async fn fixture() -> (SqliteStore, AuthenticatedPrincipal) {
        let url = format!(
            "sqlite:file:{}?mode=memory&cache=shared",
            uuid::Uuid::new_v4()
        );
        let store = SqliteStore::open(&url)
            .await
            .unwrap_or_else(|error| panic!("open publication fixture: {error}"));
        store
            .bootstrap_local_authority("46173b61-52ee-4270-bc9f-f140d64064f0", "SeiNel")
            .await
            .unwrap_or_else(|error| panic!("bootstrap publication identity: {error}"));
        store
            .create_room_for_local_operator(
                "20000000-0000-4000-8000-000000000011",
                "general",
                "General",
            )
            .await
            .unwrap_or_else(|error| panic!("create publication room: {error}"));
        store
            .acknowledge_room_publication("general", 1)
            .await
            .unwrap_or_else(|error| panic!("publish room creation baseline: {error}"));
        let principal = AuthenticatedPrincipal {
            principal_id: "operator-local-user".to_owned(),
            participant_id: "operator-local".to_owned(),
            display_name: "SeiNel".to_owned(),
            room_id: "general".to_owned(),
            client_kind: ClientKind::Browser,
            invite_scope: InviteScope::ReadWrite,
            is_operator: true,
            capabilities: CapabilitySet::local_operator(
                ClientKind::Browser,
                InviteScope::ReadWrite,
            ),
        };
        (store, principal)
    }
}
