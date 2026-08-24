use agentsassemble_domain::{AuthenticatedPrincipal, RoomEvent};
use sqlx::Row;

use crate::{PersistenceError, SqliteStore, authority::authorize_session};

#[derive(Debug, Clone, PartialEq)]
pub struct RoomCatchUp {
    pub high_water: i64,
    pub events: Vec<RoomEvent>,
}

impl SqliteStore {
    /// Fixes a durable high-water mark and reads the exact contiguous range after a snapshot.
    ///
    /// Authorization, high-water selection, and event loading share one `SQLite` read transaction.
    /// The caller must already have registered its live receiver before creating the snapshot.
    ///
    /// # Errors
    ///
    /// Rejects revoked sessions, invalid cursors, oversized finite catch-up ranges, sequence
    /// corruption, or underlying persistence failures.
    pub async fn room_subscription_catch_up(
        &self,
        principal: &AuthenticatedPrincipal,
        after_seq: i64,
        max_events: i64,
    ) -> Result<RoomCatchUp, PersistenceError> {
        if after_seq < 0 {
            return Err(PersistenceError::InvalidCursor {
                durable_last_seq: 0,
            });
        }
        let max_events = max_events.max(1);
        let mut transaction = self.pool.begin().await?;
        authorize_session(&mut transaction, principal).await?;
        let high_water = sqlx::query_scalar::<_, i64>(
            "SELECT COALESCE(MAX(seq), 0) FROM room_events WHERE room_id = ?",
        )
        .bind(&principal.room_id)
        .fetch_one(&mut *transaction)
        .await?;
        if after_seq > high_water {
            return Err(PersistenceError::InvalidCursor {
                durable_last_seq: high_water,
            });
        }
        let rows = sqlx::query(
            "SELECT seq, event_json FROM room_events \
             WHERE room_id = ? AND seq > ? AND seq <= ? ORDER BY seq LIMIT ?",
        )
        .bind(&principal.room_id)
        .bind(after_seq)
        .bind(high_water)
        .bind(max_events.saturating_add(1))
        .fetch_all(&mut *transaction)
        .await?;
        if i64::try_from(rows.len()).unwrap_or(i64::MAX) > max_events {
            return Err(PersistenceError::SubscriptionCatchUpExceeded {
                high_water,
                limit: max_events,
            });
        }
        let mut expected = after_seq.saturating_add(1);
        let mut events = Vec::with_capacity(rows.len());
        for row in rows {
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
            events.push(event);
            expected = expected.saturating_add(1);
        }
        if expected != high_water.saturating_add(1) {
            return Err(PersistenceError::SubscriptionSequenceGap {
                expected,
                found: high_water,
            });
        }
        transaction.commit().await?;
        Ok(RoomCatchUp { high_water, events })
    }
}

#[cfg(test)]
mod tests {
    use agentsassemble_domain::{AuthenticatedPrincipal, CapabilitySet, ClientKind, InviteScope};
    use serde_json::json;

    use crate::{PersistenceError, SqliteStore};

    async fn fixture() -> (SqliteStore, AuthenticatedPrincipal) {
        let store = SqliteStore::open(&format!(
            "sqlite:file:{}?mode=memory&cache=shared",
            uuid::Uuid::new_v4()
        ))
        .await
        .unwrap_or_else(|error| panic!("open catch-up fixture: {error}"));
        store
            .bootstrap_local_authority("6c99a3ee-03e8-4387-ac13-251d51c86ddd", "Host")
            .await
            .unwrap_or_else(|error| panic!("bootstrap catch-up fixture: {error}"));
        store
            .create_room_for_local_operator(
                "7bf09909-c34c-4c0d-b04e-16436b693962",
                "general",
                "General",
            )
            .await
            .unwrap_or_else(|error| panic!("create catch-up room: {error}"));
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
    async fn catch_up_fixes_one_exact_bounded_contiguous_range() {
        let (store, principal) = fixture().await;
        let snapshot = store
            .snapshot_for(&principal, 0, 200)
            .await
            .unwrap_or_else(|error| panic!("snapshot catch-up fixture: {error}"));
        assert_eq!(snapshot.last_seq, 1);
        for index in 1..=2 {
            store
                .execute_message(
                    &principal,
                    &format!("catch-up-{index}"),
                    "message.send",
                    &json!({"content": format!("message {index}")}),
                )
                .await
                .unwrap_or_else(|error| panic!("commit catch-up event: {error}"));
        }
        let catch_up = store
            .room_subscription_catch_up(&principal, snapshot.last_seq, 2)
            .await
            .unwrap_or_else(|error| panic!("load exact catch-up: {error}"));
        assert_eq!(catch_up.high_water, 3);
        assert_eq!(
            catch_up
                .events
                .iter()
                .map(|event| event.seq)
                .collect::<Vec<_>>(),
            [2, 3]
        );
        assert!(matches!(
            store
                .room_subscription_catch_up(&principal, snapshot.last_seq, 1)
                .await,
            Err(PersistenceError::SubscriptionCatchUpExceeded {
                high_water: 3,
                limit: 1,
            })
        ));
    }
}
