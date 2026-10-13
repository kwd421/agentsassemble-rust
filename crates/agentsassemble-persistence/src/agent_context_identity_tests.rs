use agentsassemble_domain::{ParticipantRole, RoomSettings, public_settings};
use serde_json::json;

use super::{
    AGENT_ID, SECOND_AGENT_ID, attached_session, authority, fixture, insert_agent, participant,
    running_authority, save_stored_session, stored_session,
};
use crate::{ProviderMessageSearchAuthority, RoomMutationAuthority::TrustedPrincipal};

#[tokio::test]
async fn role_changes_reach_live_status_and_next_api_context_without_authority_changes()
-> Result<(), Box<dyn std::error::Error>> {
    let (store, principal, _directory) = fixture().await;
    let mut session = stored_session(&store).await;
    session.public.runtime_kind = "api".into();
    session.public.provider_kind = "deepseek_api".into();
    save_stored_session(&store, &session).await;
    let mut initial = store
        .execute_message_with_turn(
            &principal,
            "initial-role-turn",
            "message.send",
            &json!({"content": "@Terra hello"}),
        )
        .await?;
    let mut assignment = initial.assignments.remove(0);
    let mut capabilities = None;
    for (index, role) in [ParticipantRole::Director, ParticipantRole::Agent]
        .into_iter()
        .enumerate()
    {
        let provider_turn = format!("role-turn-{index}");
        let start = running_authority(&store, &assignment, &provider_turn).await;
        let read = ProviderMessageSearchAuthority {
            room_id: "general",
            session_id: AGENT_ID,
            turn_id: &assignment.turn_id,
            input_up_to_seq: assignment.session.input_up_to_seq,
            turn_generation: assignment.turn_generation,
            execution_id: &assignment.execution_id,
        };
        let mut tx = store.pool.begin().await?;
        let before = crate::message_search::provider_search_principal(&mut tx, read).await?;
        tx.commit().await?;
        let original = capabilities.get_or_insert(before.capabilities);
        store
            .execute_participant_role_update(
                TrustedPrincipal(&principal),
                &format!("role-{index}"),
                &json!({"participant_id": AGENT_ID, "role": role}),
            )
            .await?;
        assert_eq!(
            stored_session(&store).await.public.active_turn_id,
            assignment.turn_id
        );
        let status = store.provider_conversation_status(read, 0).await?;
        assert_eq!(
            status
                .agents
                .iter()
                .find(|agent| agent.participant_id == AGENT_ID)
                .unwrap_or_else(|| panic!("missing agent status"))
                .role,
            role
        );
        let mut tx = store.pool.begin().await?;
        let after = crate::message_search::provider_search_principal(&mut tx, read).await?;
        tx.commit().await?;
        assert_eq!(&after.capabilities, original);
        let completed = store
            .complete_agent_turn(
                "general",
                AGENT_ID,
                authority(&start, &provider_turn, None),
                "Acknowledged.",
                "",
                None,
                &[],
            )
            .await?;
        assert!(completed.next_assignments.is_empty());
        let mut next = store
            .execute_message_with_turn(
                &principal,
                &format!("next-role-{index}"),
                "message.send",
                &json!({"content": "@Terra continue"}),
            )
            .await?;
        assignment = next.assignments.remove(0);
        let expected = format!(
            "room participant ID: {AGENT_ID}; role: {}",
            serde_json::to_string(&role)?
        );
        assert!(
            assignment.room_view.contains(&expected),
            "{}",
            assignment.room_view
        );
        assert_eq!(
            assignment.session.public.permission_mode,
            session.public.permission_mode
        );
    }
    Ok(())
}

#[tokio::test]
async fn picker_mentions_identify_self_independently_of_provider_identity_and_floor() {
    for ambient in [false, true] {
        let (store, principal, _directory) = fixture().await;
        let now = chrono::Utc::now();
        let other = participant(
            SECOND_AGENT_ID,
            "Flash",
            "agent",
            ParticipantRole::Reviewer,
            now,
        );
        let mut second = attached_session(now);
        second.public.session_id = SECOND_AGENT_ID.into();
        second.public.participant_id = SECOND_AGENT_ID.into();
        second.public.display_name = "Flash".into();
        second.provider_session_id = "provider-thread-flash".into();
        second.runtime_handle_id = "runtime-flash".into();
        insert_agent(&store, &other, &second).await;
        if ambient {
            let revision = public_settings(&RoomSettings::defaults("General"))
                .unwrap_or_else(|error| panic!("{error:?}"))
                .settings_revision;
            store
                .execute_room_settings_update(
                    TrustedPrincipal(&principal),
                    "ambient",
                    &json!({"expected_revision": revision, "conversation_mode": "ambient"}),
                )
                .await
                .unwrap_or_else(|error| panic!("{error:?}"));
        }
        let result = store.execute_message_with_turn(
            &principal, "picker-mention", "message.send",
            &json!({"content": format!("<@{SECOND_AGENT_ID}> context; <@{AGENT_ID}> please answer")}),
        ).await.unwrap_or_else(|error| panic!("{error:?}"));
        assert_eq!(result.assignments.len(), if ambient { 2 } else { 1 });
        for assigned in result.assignments {
            let id = &assigned.session.public.participant_id;
            assert_ne!(id, &assigned.session.provider_session_id);
            let expected = format!("room participant ID: {id}; role:");
            assert!(assigned.room_view.contains(&expected));
            assert!(
                assigned
                    .room_view
                    .contains("Mentions you (floor assignment is separate).")
            );
            assert!(
                !assigned
                    .room_view
                    .contains(&assigned.session.provider_session_id)
            );
            if id == AGENT_ID {
                assert!(
                    assigned
                        .room_view
                        .contains(&format!("{SECOND_AGENT_ID} (role: \"reviewer\")"))
                );
            }
        }
    }
}
