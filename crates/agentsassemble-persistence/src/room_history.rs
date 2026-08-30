use agentsassemble_domain::{
    AuthenticatedPrincipal, ROOM_HISTORY_MAX_EVENTS, RoomEvent, RoomHistoryPage,
    RoomHistoryRequest, public_event_for_principal,
};
use sqlx::Row;

use crate::{PersistenceError, SqliteStore, authority::authorize_session};

impl SqliteStore {
    /// Reads one current-principal canonical room-history page without mutation admission.
    ///
    /// Authorization, high-water selection, event loading, identity validation, and public
    /// projection share one `SQLite` read transaction.
    ///
    /// # Errors
    ///
    /// Rejects stale authority, missing history permission, corrupt event identity, or storage
    /// failure without returning a partial page.
    pub async fn room_history_page(
        &self,
        principal: &AuthenticatedPrincipal,
        request: RoomHistoryRequest,
    ) -> Result<RoomHistoryPage, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        authorize_session(&mut transaction, principal).await?;
        if !principal.capabilities.room_history {
            return Err(PersistenceError::CommandRejected {
                code: "permission_denied",
                message: "room.history permission is required.".to_owned(),
            });
        }
        if request.before_seq < 0 || !(1..=ROOM_HISTORY_MAX_EVENTS).contains(&request.limit) {
            return Err(PersistenceError::CommandRejected {
                code: "bad_request",
                message: "room.history cursor or limit is outside the supported range.".to_owned(),
            });
        }
        let limit =
            usize::try_from(request.limit).map_err(|_| PersistenceError::CommandRejected {
                code: "bad_request",
                message: "room.history limit is outside the supported range.".to_owned(),
            })?;
        let last_seq = sqlx::query_scalar::<_, i64>(
            "SELECT COALESCE(MAX(seq), 0) FROM room_events WHERE room_id = ?",
        )
        .bind(&principal.room_id)
        .fetch_one(&mut *transaction)
        .await?;
        let upper_exclusive = if request.before_seq == 0 {
            last_seq.saturating_add(1)
        } else {
            request.before_seq
        };
        let rows = sqlx::query(
            "SELECT seq, event_json FROM room_events \
             WHERE room_id = ? AND seq < ? ORDER BY seq DESC LIMIT ?",
        )
        .bind(&principal.room_id)
        .bind(upper_exclusive)
        .bind(request.limit.saturating_add(1))
        .fetch_all(&mut *transaction)
        .await?;
        let has_more_before = rows.len() > limit;
        let mut expected = last_seq.min(upper_exclusive.saturating_sub(1));
        let mut events = Vec::with_capacity(rows.len().min(limit));
        for row in rows.into_iter().take(limit) {
            let stored_seq = row.get::<i64, _>("seq");
            let event =
                serde_json::from_str::<RoomEvent>(row.get::<String, _>("event_json").as_str())?;
            if stored_seq != expected
                || event.seq != stored_seq
                || event.room_id != principal.room_id
            {
                return Err(PersistenceError::SubscriptionSequenceGap {
                    expected,
                    found: stored_seq,
                });
            }
            events.push(public_event_for_principal(&event, principal));
            expected = expected.saturating_sub(1);
        }
        events.reverse();
        let oldest_seq = events.first().map_or(0, |event| event.seq);
        transaction.commit().await?;
        Ok(RoomHistoryPage {
            events,
            oldest_seq,
            last_seq,
            has_more_before,
        })
    }
}

#[cfg(test)]
mod tests {
    use agentsassemble_domain::{
        AuthenticatedPrincipal, CapabilitySet, ClientKind, InviteScope, RoomHistoryRequest,
    };
    use serde_json::json;
    use sqlx::Row;

    use crate::{PersistenceError, SqliteStore};

    async fn fixture() -> (SqliteStore, AuthenticatedPrincipal) {
        let store = SqliteStore::open(&format!(
            "sqlite:file:{}?mode=memory&cache=shared",
            uuid::Uuid::new_v4()
        ))
        .await
        .unwrap_or_else(|error| panic!("open history fixture: {error}"));
        store
            .bootstrap_local_authority("6c99a3ee-03e8-4387-ac13-251d51c86ddd", "Host")
            .await
            .unwrap_or_else(|error| panic!("bootstrap history fixture: {error}"));
        store
            .create_room_for_local_operator(
                "7bf09909-c34c-4c0d-b04e-16436b693962",
                "general",
                "General",
            )
            .await
            .unwrap_or_else(|error| panic!("create history room: {error}"));
        let principal = AuthenticatedPrincipal {
            principal_id: "operator-local-user".to_owned(),
            participant_id: "operator-local".to_owned(),
            display_name: "Host".to_owned(),
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

    #[tokio::test]
    async fn pages_complete_history_in_chronological_bounded_windows() {
        let (store, principal) = fixture().await;
        for index in 0..250 {
            store
                .execute_message(
                    &principal,
                    &format!("message-{index}"),
                    "message.send",
                    &json!({"content": format!("message {index}")}),
                )
                .await
                .unwrap_or_else(|error| panic!("commit history event: {error}"));
        }
        let newest = store
            .room_history_page(
                &principal,
                RoomHistoryRequest {
                    before_seq: 0,
                    limit: 200,
                },
            )
            .await
            .unwrap_or_else(|error| panic!("read newest history: {error}"));
        assert_eq!(newest.events.len(), 200);
        assert_eq!(newest.oldest_seq, 52);
        assert_eq!(newest.last_seq, 251);
        assert!(newest.has_more_before);
        assert_eq!(newest.events.first().map(|event| event.seq), Some(52));
        assert_eq!(newest.events.last().map(|event| event.seq), Some(251));

        let earlier = store
            .room_history_page(
                &principal,
                RoomHistoryRequest {
                    before_seq: newest.oldest_seq,
                    limit: 200,
                },
            )
            .await
            .unwrap_or_else(|error| panic!("read earlier history: {error}"));
        assert_eq!(earlier.events.len(), 51);
        assert_eq!(earlier.oldest_seq, 1);
        assert_eq!(earlier.last_seq, 251);
        assert!(!earlier.has_more_before);
    }

    #[tokio::test]
    async fn read_projects_hidden_events_and_writes_nothing() {
        let (store, principal) = fixture().await;
        store
            .execute_message(
                &principal,
                "private-event",
                "message.send",
                &json!({"content": "private"}),
            )
            .await
            .unwrap_or_else(|error| panic!("commit private fixture event: {error}"));
        let row = sqlx::query("SELECT event_json FROM room_events WHERE room_id = ? AND seq = 2")
            .bind(&principal.room_id)
            .fetch_one(&store.pool)
            .await
            .unwrap_or_else(|error| panic!("read private fixture event: {error}"));
        let mut event: serde_json::Value =
            serde_json::from_str(row.get::<String, _>("event_json").as_str())
                .unwrap_or_else(|error| panic!("decode private fixture event: {error}"));
        event["visibility"] = json!("owner");
        event["participant_id"] = json!("somebody-else");
        sqlx::query("UPDATE room_events SET event_json = ? WHERE room_id = ? AND seq = 2")
            .bind(
                serde_json::to_string(&event)
                    .unwrap_or_else(|error| panic!("encode private fixture event: {error}")),
            )
            .bind(&principal.room_id)
            .execute(&store.pool)
            .await
            .unwrap_or_else(|error| panic!("write private fixture event: {error}"));
        let counts_before = sqlx::query(
            "SELECT (SELECT COUNT(*) FROM room_events) AS events, \
             (SELECT COUNT(*) FROM command_results) AS commands",
        )
        .fetch_one(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("read history counts before: {error}"));

        let page = store
            .room_history_page(
                &principal,
                RoomHistoryRequest {
                    before_seq: 0,
                    limit: 200,
                },
            )
            .await
            .unwrap_or_else(|error| panic!("read projected history: {error}"));
        assert_eq!(page.events[1].event_type, "event_hidden");
        assert!(page.events[1].content.is_none());
        let counts_after = sqlx::query(
            "SELECT (SELECT COUNT(*) FROM room_events) AS events, \
             (SELECT COUNT(*) FROM command_results) AS commands",
        )
        .fetch_one(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("read history counts after: {error}"));
        assert_eq!(
            counts_after.get::<i64, _>("events"),
            counts_before.get::<i64, _>("events")
        );
        assert_eq!(
            counts_after.get::<i64, _>("commands"),
            counts_before.get::<i64, _>("commands")
        );

        let mut denied = principal;
        denied.capabilities.room_history = false;
        assert!(matches!(
            store
                .room_history_page(
                    &denied,
                    RoomHistoryRequest {
                        before_seq: 0,
                        limit: 200,
                    },
                )
                .await,
            Err(PersistenceError::CommandRejected {
                code: "permission_denied",
                ..
            })
        ));
    }
}
