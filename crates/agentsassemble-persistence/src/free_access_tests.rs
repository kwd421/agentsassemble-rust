use super::*;
use crate::human_session_authority_tests::local_operator_principal;
use agentsassemble_domain::RoomInputDeliveryKind;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[tokio::test]
async fn free_access_targets_all_mentions_and_preserves_inflight_mode_transitions() -> TestResult {
    for runtime in ["live_cli", "api"] {
        let (store, principal, _directory) = fixture().await;
        insert_second_agent(&store).await;
        let mut session = stored_session(&store, super::super::AGENT_ID).await;
        session.public.runtime_kind = runtime.into();
        // Use a committed update; provider kind only changes prompt preparation in this fixture.
        sqlx::query("UPDATE agent_sessions SET session_json=? WHERE session_id=?")
            .bind(serde_json::to_string(&session)?)
            .bind(&session.public.session_id)
            .execute(&store.pool)
            .await?;
        let revision = public_settings(&RoomSettings::defaults("General"))?.settings_revision;
        let mode = update_mode(&store, &principal, "free", &revision, "free_access").await?;
        let ordinary = send_message(&store, &principal, "ordinary", "Nobody is addressed").await;
        assert!(ordinary.assignments.is_empty());
        for id in [super::super::AGENT_ID, SECOND_AGENT_ID] {
            assert!(stored_session(&store, id).await.pending_inputs.is_empty());
        }
        let targeted = send_message(
            &store,
            &principal,
            "targeted",
            "@Terra and <@ Flash > answer; @Terra again",
        )
        .await;
        assert_eq!(targeted.assignments.len(), 2);
        for assignment in &targeted.assignments {
            assert_eq!(
                assignment.delivery_kind,
                RoomInputDeliveryKind::FreeAccessMention
            );
            assert!(assignment.provider_input.contains("explicitly mentioned"));
        }
        send_message(&store, &principal, "queued1", "@Terra first queued mention").await;
        send_message(
            &store,
            &principal,
            "queued2",
            "@Terra second queued mention",
        )
        .await;
        assert!(
            stored_session(&store, SECOND_AGENT_ID)
                .await
                .pending_inputs
                .is_empty()
        );
        let before = stored_session(&store, super::super::AGENT_ID).await;
        let ordered = update_mode(
            &store,
            &principal,
            "ordered",
            mode.result["room_settings"]["settings_revision"]
                .as_str()
                .ok_or("revision")?,
            "ordered",
        )
        .await?;
        update_mode(
            &store,
            &principal,
            "free-again",
            ordered.result["room_settings"]["settings_revision"]
                .as_str()
                .ok_or("revision")?,
            "free_access",
        )
        .await?;
        let after = stored_session(&store, super::super::AGENT_ID).await;
        assert_eq!(before.pending_inputs, after.pending_inputs);
        assert_eq!(before.inflight_inputs, after.inflight_inputs);
        for assignment in &targeted.assignments {
            let done =
                decline_assignment(&store, assignment, "done", "nothing_useful_to_add").await;
            if assignment.session.public.session_id == super::super::AGENT_ID {
                assert_eq!(done.next_assignments.len(), 1);
                assert_eq!(done.next_assignments[0].session.inflight_inputs.len(), 1);
                assert_eq!(done.next_assignments[0].session.pending_inputs.len(), 1);
            }
        }
    }
    Ok(())
}

#[tokio::test]
async fn free_access_thirteenth_ai_message_cannot_route_and_human_resets_chain() -> TestResult {
    let (store, principal, _directory) = fixture().await;
    insert_second_agent(&store).await;
    let revision = public_settings(&RoomSettings::defaults("General"))?.settings_revision;
    update_mode(&store, &principal, "free", &revision, "free_access").await?;
    let mut ai = principal.clone();
    ai.principal_id = SECOND_AGENT_ID.into();
    ai.participant_id = SECOND_AGENT_ID.into();
    ai.display_name = "Flash".into();
    for index in 1..12 {
        assert!(
            send_message(&store, &ai, &format!("ai-{index}"), "ordinary AI speech")
                .await
                .assignments
                .is_empty()
        );
    }
    let twelfth = send_message(&store, &ai, "ai-12", "@Terra respond").await;
    assert_eq!(twelfth.assignments.len(), 1);
    decline_assignment(
        &store,
        &twelfth.assignments[0],
        "done",
        "nothing_useful_to_add",
    )
    .await;
    let thirteenth = send_message(&store, &ai, "ai-13", "@Terra respond again").await;
    assert!(thirteenth.assignments.is_empty());
    assert!(
        stored_session(&store, super::super::AGENT_ID)
            .await
            .pending_inputs
            .is_empty()
    );
    assert_eq!(
        thirteenth.outcome.event.content.as_deref(),
        Some("@Terra respond again")
    );
    send_message(
        &store,
        &principal,
        "human",
        "Human resumes the conversation",
    )
    .await;
    assert_eq!(
        send_message(&store, &ai, "ai-reset", "@Terra respond")
            .await
            .assignments
            .len(),
        1
    );
    Ok(())
}

#[tokio::test]
async fn free_access_companion_is_silent_until_addressed() -> TestResult {
    let (store, session, now) = crate::attendee::connection_tests::fixture().await?;
    let connection = store
        .claim_attendee_connection(&session, uuid::Uuid::new_v4(), None, now)
        .await?
        .authorization;
    store
        .record_attendee_ready(&connection, &crate::attendee::ready_tests::report(), now)
        .await?;
    let principal = local_operator_principal();
    let revision =
        public_settings(&store.snapshot("general", 0, 1).await?.settings)?.settings_revision;
    update_mode(&store, &principal, "free", &revision, "free_access").await?;
    let ordinary = send_message(&store, &principal, "ordinary", "No addressee").await;
    assert!(ordinary.assignments.is_empty());
    let id = &session.principal().participant_id;
    assert!(stored_session(&store, id).await.pending_inputs.is_empty());
    assert!(
        store
            .deliver_attendee_turn(&connection, now)
            .await?
            .is_none()
    );
    let called = send_message(&store, &principal, "mentioned", &format!("@{id} answer")).await;
    assert_eq!(called.assignments.len(), 1);
    assert!(
        store
            .deliver_attendee_turn(&connection, now)
            .await?
            .is_some()
    );
    Ok(())
}

#[tokio::test]
async fn free_access_mentions_survive_schema_upgrade_and_reopen_until_exact_read() -> TestResult {
    use sha2::{Digest, Sha256};
    let (store, principal, directory) = fixture().await;
    let before = store.snapshot("general", 0, 200).await?;
    sqlx::query("DROP TABLE connector_mentions")
        .execute(&store.pool)
        .await?;
    sqlx::query("UPDATE runtime_metadata SET value='88' WHERE key='schema_version'")
        .execute(&store.pool)
        .await?;
    store.pool.close().await;
    drop(store);
    let path = directory.path().join("runtime.sqlite3");
    let store = SqliteStore::open_path(&path).await?;
    let after = store.snapshot("general", 0, 200).await?;
    assert_eq!(before.events, after.events);
    assert_eq!(before.participants, after.participants);
    assert_eq!(before.settings, after.settings);
    let revision = public_settings(&after.settings)?.settings_revision;
    update_mode(&store, &principal, "free", &revision, "free_access").await?;
    let manager = store
        .authorize_local_room_manager(
            "general",
            agentsassemble_domain::LOCAL_OPERATOR_USER_ID,
            agentsassemble_domain::LOCAL_OPERATOR_PARTICIPANT_ID,
        )
        .await?;
    let now = chrono::Utc::now();
    let invite = store
        .create_connector_invite(
            &crate::RoomManagerAuthority::Local(manager),
            uuid::Uuid::new_v4(),
            agentsassemble_domain::InviteScope::ReadOnly,
            now,
        )
        .await?;
    let fingerprint = Sha256::digest(invite.invite_bearer.as_bytes()).into();
    let joined = store
        .admit_connector(&fingerprint, &[3; 32], uuid::Uuid::new_v4(), "Reader", now)
        .await?;
    let first = send_message(&store, &principal, "first", "@Reader first").await;
    let second = send_message(&store, &principal, "second", "@Reader second").await;
    store.pool.close().await;
    drop(store);
    let reopened = SqliteStore::open_path(&path).await?;
    let fingerprint = Sha256::digest(joined.session_bearer.as_bytes()).into();
    let authorization = reopened
        .authorize_connector_session(&fingerprint, now)
        .await?;
    let unread = reopened
        .connector_mentions(&authorization, &[], false)
        .await?;
    assert_eq!(
        unread["unread_mentions"]
            .as_array()
            .ok_or("mentions")?
            .len(),
        2
    );
    reopened
        .connector_mentions(&authorization, &[first.outcome.event.id], false)
        .await?;
    let unread = reopened
        .connector_mentions(&authorization, &[], false)
        .await?;
    assert_eq!(
        unread["unread_mentions"]
            .as_array()
            .ok_or("mentions")?
            .len(),
        1
    );
    assert_eq!(
        unread["unread_mentions"][0]["event_id"],
        second.outcome.event.id
    );
    Ok(())
}

#[tokio::test]
async fn entering_free_access_drains_previously_accepted_inputs_without_reclassification()
-> TestResult {
    let (store, principal, _directory) = fixture().await;
    let active = send_message(&store, &principal, "active", "@Terra first").await;
    send_message(&store, &principal, "queued", "Earlier ordinary input").await;
    let before = stored_session(&store, super::super::AGENT_ID).await;
    let revision = public_settings(&RoomSettings::defaults("General"))?.settings_revision;
    update_mode(&store, &principal, "free", &revision, "free_access").await?;
    let after = stored_session(&store, super::super::AGENT_ID).await;
    assert_eq!(before.pending_inputs, after.pending_inputs);
    assert_eq!(before.inflight_inputs, after.inflight_inputs);
    let completed = decline_assignment(
        &store,
        &active.assignments[0],
        "first-done",
        "nothing_useful_to_add",
    )
    .await;
    assert_eq!(completed.next_assignments.len(), 1);
    let retained = &completed.next_assignments[0];
    assert_eq!(retained.session.inflight_inputs, before.pending_inputs);
    assert!(
        retained
            .provider_input
            .contains("queued before the mode changed")
    );
    assert!(
        send_message(&store, &principal, "ordinary-free", "New ordinary input")
            .await
            .assignments
            .is_empty()
    );
    assert!(
        stored_session(&store, super::super::AGENT_ID)
            .await
            .pending_inputs
            .is_empty()
    );
    Ok(())
}

#[tokio::test]
async fn connector_backlog_pages_overflow_and_terminal_cleanup_are_independent_of_recent_history()
-> TestResult {
    use sha2::{Digest, Sha256};
    for terminal in ["leave", "remove", "room"] {
        let (store, principal, _directory) = fixture().await;
        let revision = public_settings(&RoomSettings::defaults("General"))?.settings_revision;
        update_mode(&store, &principal, "free", &revision, "free_access").await?;
        let manager = store
            .authorize_local_room_manager(
                "general",
                &principal.principal_id,
                &principal.participant_id,
            )
            .await?;
        let now = chrono::Utc::now();
        let invite = store
            .create_connector_invite(
                &crate::RoomManagerAuthority::Local(manager),
                uuid::Uuid::new_v4(),
                agentsassemble_domain::InviteScope::ReadOnly,
                now,
            )
            .await?;
        let joined = store
            .admit_connector(
                &Sha256::digest(invite.invite_bearer.as_bytes()).into(),
                &[3; 32],
                uuid::Uuid::new_v4(),
                "Reader",
                now,
            )
            .await?;
        let authorization = joined.authorization;
        let mut ids = Vec::new();
        for index in 0..205 {
            ids.push(
                send_message(
                    &store,
                    &principal,
                    &format!("mention-{index}"),
                    "@Reader read me",
                )
                .await
                .outcome
                .event
                .id,
            );
        }
        for index in 0..60 {
            send_message(
                &store,
                &principal,
                &format!("ordinary-{index}"),
                "recent ordinary message",
            )
            .await;
        }
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM connector_mentions")
            .fetch_one(&store.pool)
            .await?;
        assert_eq!(count, 200);
        let pending = store.connector_mentions(&authorization, &[], false).await?;
        assert_eq!(pending["mention_overflow"], true);
        assert_eq!(pending["unread_mentions"][0]["event_id"], ids[5]);
        for page in 0..4 {
            let read = store.connector_mentions(&authorization, &[], true).await?;
            assert_eq!(read["mention_messages"].as_array().ok_or("page")?.len(), 50);
            assert_eq!(read["unread_mentions"][0]["event_id"], ids[5 + page * 50]);
            assert_eq!(read["mention_overflow"], page == 0);
        }
        assert_eq!(
            store.connector_mentions(&authorization, &[], false).await?["unread_mentions"],
            json!([])
        );
        send_message(
            &store,
            &principal,
            "pending-at-end",
            "@Reader before leaving",
        )
        .await;
        match terminal {
            "leave" => {
                store
                    .leave_connector_session(&authorization, "leave")
                    .await?;
            }
            "remove" => {
                let mut tx = store.pool.begin().await?;
                crate::participant_removal::revoke_participant_access(
                    &mut tx,
                    "general",
                    &authorization.principal().participant_id,
                )
                .await?;
                tx.commit().await?;
            }
            _ => {
                let mut tx = store.pool.begin().await?;
                crate::room_lifecycle::revoke_room_access(&mut tx, "general").await?;
                tx.commit().await?;
            }
        }
        send_message(&store, &principal, "after-end", "@Reader after leaving").await;
        for table in ["connector_mentions", "connector_mention_overflow"] {
            let count: i64 =
                sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT count(*) FROM {table}")))
                    .fetch_one(&store.pool)
                    .await?;
            assert_eq!(count, 0, "terminal {terminal}: {table}");
        }
    }
    Ok(())
}

#[tokio::test]
async fn connector_delivery_survives_reopen_and_legacy_rows_require_resync() -> TestResult {
    use sha2::{Digest, Sha256};
    let (store, principal, directory) = fixture().await;
    let manager = store
        .authorize_local_room_manager(
            "general",
            &principal.principal_id,
            &principal.participant_id,
        )
        .await?;
    let now = chrono::Utc::now();
    let invite = store
        .create_connector_invite(
            &crate::RoomManagerAuthority::Local(manager),
            uuid::Uuid::new_v4(),
            agentsassemble_domain::InviteScope::ReadOnly,
            now,
        )
        .await?;
    let joined = store
        .admit_connector(
            &Sha256::digest(invite.invite_bearer.as_bytes()).into(),
            &[3; 32],
            uuid::Uuid::new_v4(),
            "Reader",
            now,
        )
        .await?;
    let ordinary = send_message(&store, &principal, "ordinary", "ordinary")
        .await
        .outcome
        .event
        .seq;
    let revision =
        public_settings(&store.snapshot("general", 0, 200).await?.settings)?.settings_revision;
    update_mode(&store, &principal, "free", &revision, "free_access").await?;
    let silent = send_message(&store, &principal, "silent", "silent")
        .await
        .outcome
        .event
        .seq;
    let path = directory.path().join("runtime.sqlite3");
    store.pool.close().await;
    drop(store);
    let store = SqliteStore::open_path(&path).await?;
    assert_eq!(
        store
            .connector_ordinary_messages(&joined.authorization, &[ordinary, silent])
            .await?,
        vec![ordinary]
    );
    // A genuine v90 upgrade has no acceptance facts for old rows. Never invent
    // those facts from the current room mode, even when it differs from arrival.
    sqlx::raw_sql("DROP TABLE connector_message_delivery; DROP TABLE connector_mention_overflow; UPDATE runtime_metadata SET value='90' WHERE key='schema_version'")
        .execute(&store.pool).await?;
    store.pool.close().await;
    drop(store);
    let store = SqliteStore::open_path(&path).await?;
    assert!(
        matches!(store.connector_ordinary_messages(&joined.authorization, &[ordinary,silent]).await,
        Err(crate::PersistenceError::CommandRejected { code, .. }) if code == "connector_resync_required")
    );
    let later = send_message(&store, &principal, "later", "still silent")
        .await
        .outcome
        .event
        .seq;
    assert!(
        store
            .connector_ordinary_messages(&joined.authorization, &[later])
            .await?
            .is_empty()
    );
    Ok(())
}
