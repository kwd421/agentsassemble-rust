use super::*;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[tokio::test]
async fn addressed_input_keeps_immutable_target_after_rename() -> TestResult {
    let (store, principal, directory) = fixture().await;
    three_agents(&store).await;
    let first = store
        .execute_message_with_turn(
            &principal,
            "source",
            "message.send",
            &json!({"content": "@Terra answer this"}),
        )
        .await?;
    let start = running_authority(&store, &first.assignments[0], "renamed").await;
    store
        .execute_agent_profile_update(
            TrustedPrincipal(&principal),
            "rename",
            &json!({"agent_id": AGENT_ID, "display_name": "Orion"}),
        )
        .await?;
    // The former alias is reused by another configured session, then persisted across restart.
    store
        .execute_agent_profile_update(
            TrustedPrincipal(&principal),
            "reuse-name",
            &json!({"agent_id": SECOND_AGENT_ID, "display_name": "Terra"}),
        )
        .await?;
    store.pool.close().await;
    drop(store);
    let store = SqliteStore::open_path(&directory.path().join("runtime.sqlite3")).await?;
    let quarantine = store
        .mark_provider_turn_recovery_required(&start, None)
        .await?;
    assert!(quarantine.next_assignments.is_empty());
    let late = store
        .complete_agent_turn(
            "general",
            AGENT_ID,
            authority(&start, "renamed", None),
            "The original target's answer",
            "",
            None,
        )
        .await?;
    assert_eq!(
        late.events
            .iter()
            .filter(|event| event.event_type == "message_final")
            .count(),
        1
    );
    Ok(())
}

#[tokio::test]
async fn renamed_target_without_alias_reuse_is_not_handed_off() -> TestResult {
    let (store, principal, _directory) = fixture().await;
    three_agents(&store).await;
    let first = store
        .execute_message_with_turn(
            &principal,
            "source",
            "message.send",
            &json!({"content": "@Terra answer this"}),
        )
        .await?;
    let start = running_authority(&store, &first.assignments[0], "renamed").await;
    store
        .execute_agent_profile_update(
            TrustedPrincipal(&principal),
            "rename",
            &json!({"agent_id": AGENT_ID, "display_name": "Orion"}),
        )
        .await?;
    let quarantine = store
        .mark_provider_turn_recovery_required(&start, None)
        .await?;
    assert!(
        quarantine.next_assignments.is_empty(),
        "renamed direct input handed to unrelated speaker"
    );
    let late = store
        .complete_agent_turn(
            "general",
            AGENT_ID,
            authority(&start, "renamed", None),
            "The original target's answer",
            "",
            None,
        )
        .await?;
    assert!(event_types(&late.events).contains(&"message_final"));
    Ok(())
}

#[tokio::test]
async fn target_removal_cannot_turn_addressed_input_into_ordinary_work() -> TestResult {
    let (store, principal, _directory) = fixture().await;
    three_agents(&store).await;
    let first = store
        .execute_message_with_turn(
            &principal,
            "source",
            "message.send",
            &json!({"content": "@Terra answer this"}),
        )
        .await?;
    // Model the removal boundary after exact runtime cleanup. Routing identity must outlive
    // session deletion; the retained source event is still canonical room history.
    sqlx::query("DELETE FROM agent_sessions WHERE room_id = 'general' AND session_id = ?")
        .bind(AGENT_ID)
        .execute(&store.pool)
        .await?;
    let mut tx = store.pool.begin().await?;
    let (_, settings) =
        crate::room_turns::support::load_room_with_settings(&mut tx, "general").await?;
    let handed = crate::room_turns::scheduler::route_released_floor(
        &mut tx,
        &settings,
        &first.outcome.event,
    )
    .await?;
    assert!(
        !handed,
        "removed direct target changed the input classification"
    );
    tx.commit().await?;
    assert!(store.assign_pending_turn("general").await?.is_none());
    Ok(())
}

#[tokio::test]
async fn newly_matching_name_cannot_capture_an_ordinary_released_input() -> TestResult {
    let (store, principal, _directory) = fixture().await;
    let first = store
        .execute_message_with_turn(
            &principal,
            "source",
            "message.send",
            &json!({"content": "Ask @Orion when they join"}),
        )
        .await?;
    let start = running_authority(&store, &first.assignments[0], "ordinary").await;
    three_agents(&store).await;
    store
        .execute_agent_profile_update(
            TrustedPrincipal(&principal),
            "rename",
            &json!({"agent_id": AGENT_ID, "display_name": "Orion"}),
        )
        .await?;
    let quarantine = store
        .mark_provider_turn_recovery_required(&start, None)
        .await?;
    assert_eq!(
        quarantine.next_assignments.len(),
        1,
        "ordinary input was reclassified as direct"
    );
    assert_ne!(
        quarantine.next_assignments[0].session.public.session_id,
        AGENT_ID
    );
    Ok(())
}

#[tokio::test]
async fn upgrade_recovers_routing_from_original_names_before_startup_release() -> TestResult {
    for (addressed, stopped) in [(false, false), (true, false), (false, true), (true, true)] {
        let (store, principal, directory) = fixture().await;
        // Legacy canonical state history is the upgrade authority, not the current profile.
        let mut tx = store.pool.begin().await?;
        let original = crate::agent_lifecycle::load_session(&mut tx, "general", AGENT_ID).await?;
        crate::room_turns::support::session_state_event(&mut tx, &original).await?;
        tx.commit().await?;
        let content = if addressed {
            "@Terra answer this"
        } else {
            "Ask @Orion when they join"
        };
        let first = store
            .execute_message_with_turn(
                &principal,
                "source",
                "message.send",
                &json!({"content": content}),
            )
            .await?;
        let start = running_authority(&store, &first.assignments[0], "legacy").await;
        if stopped {
            stop(&store, &principal, AGENT_ID).await;
        } else {
            store
                .mark_provider_turn_recovery_required(&start, None)
                .await?;
        }
        store
            .execute_agent_profile_update(
                TrustedPrincipal(&principal),
                "rename",
                &json!({"agent_id": AGENT_ID, "display_name": "Orion"}),
            )
            .await?;
        three_agents(&store).await;
        sqlx::query("DROP TABLE ordered_input_routes")
            .execute(&store.pool)
            .await?;
        if stopped {
            sqlx::query("ALTER TABLE provider_turn_executions DROP COLUMN released_input_ids")
                .execute(&store.pool)
                .await?;
        } else {
            sqlx::query("UPDATE provider_turn_executions SET released_input_ids = NULL")
                .execute(&store.pool)
                .await?;
        }
        sqlx::query("UPDATE runtime_metadata SET value = ? WHERE key = 'schema_version'")
            .bind(if stopped { "77" } else { "78" })
            .execute(&store.pool)
            .await?;
        store.pool.close().await;
        drop(store);
        let store = SqliteStore::open_path(&directory.path().join("runtime.sqlite3")).await?;
        let page = store.load_provider_turn_reconciliation_page(None).await?;
        assert_eq!(
            page.candidates
                .iter()
                .filter(|candidate| candidate.execution.phase
                    == crate::ProviderTurnExecutionPhase::Assigned)
                .count(),
            usize::from(!addressed)
        );
        let classification: bool = sqlx::query_scalar(
            "SELECT directly_addressed FROM ordered_input_routes WHERE event_seq = ?",
        )
        .bind(first.outcome.event.seq)
        .fetch_one(&store.pool)
        .await?;
        assert_eq!(classification, addressed);
    }
    Ok(())
}

#[tokio::test]
async fn first_ordered_route_after_ambient_decline_has_release_identity() -> TestResult {
    let (store, principal, _directory) = fixture().await;
    set_mode(&store, &principal, "ambient").await?;
    let first = store
        .execute_message_with_turn(
            &principal,
            "source",
            "message.send",
            &json!({"content": "An ordinary question"}),
        )
        .await?;
    let start = running_authority(&store, &first.assignments[0], "ambient").await;
    three_agents(&store).await;
    set_mode(&store, &principal, "ordered").await?;
    let declined = store
        .decline_agent_turn(
            "general",
            AGENT_ID,
            authority(&start, "ambient", None),
            "nothing_useful_to_add",
        )
        .await?;
    assert_eq!(declined.next_assignments.len(), 1);
    let next = running_authority(&store, &declined.next_assignments[0], "ordered").await;
    let quarantine = store
        .mark_provider_turn_recovery_required(&next, None)
        .await?;
    assert_eq!(quarantine.next_assignments.len(), 1);
    assert_ne!(
        quarantine.next_assignments[0].session.public.session_id,
        next.session_id
    );
    Ok(())
}

async fn set_mode(
    store: &SqliteStore,
    principal: &AuthenticatedPrincipal,
    mode: &str,
) -> TestResult {
    let mut tx = store.pool.begin().await?;
    let (_, settings) =
        crate::room_turns::support::load_room_with_settings(&mut tx, "general").await?;
    tx.commit().await?;
    let revision = agentsassemble_domain::public_settings(&settings)?.settings_revision;
    store
        .execute_room_settings_update(
            TrustedPrincipal(principal),
            mode,
            &json!({"expected_revision": revision, "conversation_mode": mode}),
        )
        .await?;
    Ok(())
}
