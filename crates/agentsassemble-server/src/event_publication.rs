use agentsassemble_persistence::{PersistenceError, SqliteStore};
use tokio::sync::broadcast;

const RETRY_INITIAL_DELAY: std::time::Duration = std::time::Duration::from_millis(250);
const RETRY_MAX_DELAY: std::time::Duration = std::time::Duration::from_secs(5);
const RETRY_MAX_CONSECUTIVE_FAILURES: u8 = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use]
pub(crate) enum PublicationAttempt {
    Drained,
    Retry,
}

pub(crate) struct PublicationRetry {
    deadline: Option<tokio::time::Instant>,
    next_delay: std::time::Duration,
    consecutive_failures: u8,
}

impl PublicationRetry {
    pub(crate) fn new(attempt: PublicationAttempt) -> Self {
        let mut owner = Self {
            deadline: None,
            next_delay: RETRY_INITIAL_DELAY,
            consecutive_failures: 0,
        };
        owner.record(attempt);
        owner
    }

    pub(crate) fn record(&mut self, attempt: PublicationAttempt) {
        match attempt {
            PublicationAttempt::Drained => {
                self.deadline = None;
                self.next_delay = RETRY_INITIAL_DELAY;
                self.consecutive_failures = 0;
            }
            PublicationAttempt::Retry => {
                self.consecutive_failures = self.consecutive_failures.saturating_add(1);
                if self.consecutive_failures < RETRY_MAX_CONSECUTIVE_FAILURES {
                    self.deadline = Some(tokio::time::Instant::now() + self.next_delay);
                    self.next_delay = self.next_delay.saturating_mul(2).min(RETRY_MAX_DELAY);
                } else {
                    self.deadline = None;
                }
            }
        }
    }

    pub(crate) const fn is_armed(&self) -> bool {
        self.deadline.is_some()
    }

    pub(crate) async fn wait(&self) {
        match self.deadline {
            Some(deadline) => tokio::time::sleep_until(deadline).await,
            None => std::future::pending().await,
        }
    }
}

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

pub(crate) async fn publish_durable_room_events(
    store: &SqliteStore,
    events: &broadcast::Sender<agentsassemble_domain::RoomEvent>,
    room_id: &str,
) -> PublicationAttempt {
    match drain_room_publications(store, events, room_id).await {
        Ok(()) => PublicationAttempt::Drained,
        Err(error) => {
            tracing::error!(
                error = ?error,
                room_id,
                "durable room-event publication failed"
            );
            PublicationAttempt::Retry
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
                1,
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
                1,
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
    async fn missing_room_publication_requires_retry() {
        let (store, _) = fixture().await;
        let (sender, _) = tokio::sync::broadcast::channel(1);
        assert_eq!(
            super::publish_durable_room_events(&store, &sender, "missing-room").await,
            super::PublicationAttempt::Retry
        );
    }

    #[tokio::test(start_paused = true)]
    async fn publication_retry_is_failure_only_exhaustible_and_resettable() {
        let now = tokio::time::Instant::now();
        let mut retry = super::PublicationRetry::new(super::PublicationAttempt::Drained);
        assert!(!retry.is_armed());
        let expected_delays = [250, 500, 1_000, 2_000, 4_000, 5_000, 5_000];
        for expected_delay_ms in expected_delays {
            retry.record(super::PublicationAttempt::Retry);
            assert_eq!(
                retry.deadline,
                Some(now + std::time::Duration::from_millis(expected_delay_ms))
            );
        }
        retry.record(super::PublicationAttempt::Retry);
        assert!(!retry.is_armed());
        assert_eq!(
            retry.consecutive_failures,
            super::RETRY_MAX_CONSECUTIVE_FAILURES
        );
        retry.record(super::PublicationAttempt::Retry);
        assert!(!retry.is_armed());
        retry.record(super::PublicationAttempt::Drained);
        assert!(!retry.is_armed());
        assert_eq!(retry.consecutive_failures, 0);
        retry.record(super::PublicationAttempt::Retry);
        assert_eq!(retry.deadline, Some(now + super::RETRY_INITIAL_DELAY));
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
