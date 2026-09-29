use super::{AGENT_ID, authority, fixture, running_authority, save_stored_session, stored_session};
use agentsassemble_domain::{AgentRuntimeStatus, AgentSessionStatus, MAX_ROOM_VIEW_CHARACTERS};
use serde_json::json;

#[tokio::test]
async fn reply_observation_resolves_old_sources_after_edit_or_delete() {
    for deleted in [false, true] {
        let (store, principal, _directory) = fixture().await;
        let source = store
            .execute_message_with_turn(
                &principal,
                "source",
                "message.send",
                &json!({"content": "@Terra original secret"}),
            )
            .await
            .unwrap_or_else(|error| panic!("source: {error}"));
        let assignment = &source.assignments[0];
        let running = running_authority(&store, assignment, "first").await;
        store
            .complete_agent_turn(
                "general",
                AGENT_ID,
                authority(&running, "first", None),
                "Acknowledged",
                "",
                None,
            )
            .await
            .unwrap_or_else(|error| panic!("complete: {error}"));
        let mut session = stored_session(&store).await;
        assert!(session.public.last_provider_sync_seq >= source.outcome.event.seq);
        session.public.enabled = false;
        session.public.status = AgentSessionStatus::Unavailable;
        session.public.runtime_status = AgentRuntimeStatus::Stopped;
        session.public.provider_session_active = false;
        save_stored_session(&store, &session).await;
        let reply = store.execute_message_with_turn(&principal, "reply", "message.send",
            &json!({"content": "@Terra explain this", "reply_to_event_id": source.outcome.event.id})).await
            .unwrap_or_else(|error| panic!("reply: {error}"));
        assert!(reply.assignments.is_empty());
        let payload = if deleted {
            json!({"event_id": source.outcome.event.id})
        } else {
            json!({"event_id": source.outcome.event.id, "content": "가".repeat(250)})
        };
        store
            .execute_message_mutation(
                &principal,
                "change-source",
                if deleted {
                    "message.delete"
                } else {
                    "message.edit"
                },
                &payload,
            )
            .await
            .unwrap_or_else(|error| panic!("mutate source: {error}"));
        let mut session = stored_session(&store).await;
        session.public.enabled = true;
        session.public.status = AgentSessionStatus::Attached;
        session.public.runtime_status = AgentRuntimeStatus::Idle;
        session.public.provider_session_active = true;
        save_stored_session(&store, &session).await;
        let commit = store
            .assign_pending_turn("general")
            .await
            .unwrap_or_else(|error| panic!("assign: {error}"))
            .unwrap_or_else(|| panic!("pending assignment"));
        let view = &commit.next_assignments[0].room_view;
        assert!(view.chars().count() <= MAX_ROOM_VIEW_CHARACTERS);
        assert!(!view.contains("original secret"));
        assert!(view.contains(&format!("Reply to event `{}`", source.outcome.event.id)));
        if deleted {
            assert!(view.contains("source unavailable or deleted"));
            assert!(!view.contains(" by "));
        } else {
            assert!(view.contains(&format!(
                "by {}:",
                source.outcome.event.display_name.as_deref().unwrap_or("")
            )));
            assert!(view.contains(&format!("{}…", "가".repeat(200))));
            assert!(!view.contains(&"가".repeat(201)));
        }
    }
}
