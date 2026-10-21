use agentsassemble_domain::{
    AuthenticatedPrincipal, CapabilitySet, ClientKind, InviteScope, ParticipantRole, RoomSettings,
    public_settings,
};
use serde_json::json;

use super::{
    AGENT_ID, SECOND_AGENT_ID, attached_session, authority, fixture, insert_agent, participant,
    running_authority, save_stored_session, stored_session,
};
use crate::{ProviderMessageSearchAuthority, RoomMutationAuthority::TrustedPrincipal, SqliteStore};

async fn add_human(
    store: &SqliteStore,
    id: &str,
    display_name: &str,
) -> Result<AuthenticatedPrincipal, Box<dyn std::error::Error>> {
    let now = chrono::Utc::now();
    let human = participant(id, display_name, "human", ParticipantRole::Human, now);
    sqlx::query(
        "INSERT INTO participants(room_id, participant_id, participant_json) VALUES (?, ?, ?)",
    )
    .bind("general")
    .bind(id)
    .bind(serde_json::to_string(&human)?)
    .execute(&store.pool)
    .await?;
    Ok(AuthenticatedPrincipal {
        principal_id: format!("{id}-user"),
        participant_id: id.to_owned(),
        display_name: display_name.to_owned(),
        room_id: "general".to_owned(),
        client_kind: ClientKind::Browser,
        invite_scope: InviteScope::ReadWrite,
        is_operator: false,
        capabilities: CapabilitySet::for_principal(
            ClientKind::Browser,
            InviteScope::ReadWrite,
            false,
        ),
    })
}

#[tokio::test]
async fn people_with_the_same_display_name_render_with_distinct_participant_ids()
-> Result<(), Box<dyn std::error::Error>> {
    let (store, _principal, _directory) = fixture().await;
    let first = add_human(&store, "human-same-1", "Nel Le").await?;
    let second = add_human(&store, "human-same-2", "Nel Le").await?;
    let mut session = stored_session(&store).await;
    session.public.enabled = false;
    session.public.status = agentsassemble_domain::AgentSessionStatus::Unavailable;
    session.public.runtime_status = agentsassemble_domain::AgentRuntimeStatus::Stopped;
    session.public.provider_session_active = false;
    save_stored_session(&store, &session).await;
    store
        .execute_message_with_turn(
            &first,
            "same-name-first",
            "message.send",
            &json!({"content": "first note"}),
        )
        .await?;
    store
        .execute_message_with_turn(
            &second,
            "same-name-second",
            "message.send",
            &json!({"content": "second note"}),
        )
        .await?;
    let mut session = stored_session(&store).await;
    session.public.enabled = true;
    session.public.status = agentsassemble_domain::AgentSessionStatus::Attached;
    session.public.runtime_status = agentsassemble_domain::AgentRuntimeStatus::Idle;
    session.public.provider_session_active = true;
    save_stored_session(&store, &session).await;
    let assigned = store
        .execute_message_with_turn(
            &first,
            "same-name-ask",
            "message.send",
            &json!({"content": "@Terra compare"}),
        )
        .await?;
    let view = &assigned.assignments[0].room_view;
    assert!(view.contains("Nel Le <@human-same-1> [event "), "{view}");
    assert!(view.contains("Nel Le <@human-same-2> [event "), "{view}");
    assert!(view.contains("first note"), "{view}");
    assert!(view.contains("second note"), "{view}");
    Ok(())
}

#[tokio::test]
async fn room_view_people_line_lists_joined_humans_with_their_participant_ids()
-> Result<(), Box<dyn std::error::Error>> {
    let (store, principal, _directory) = fixture().await;
    add_human(&store, "human-nel", "Nel Le").await?;
    add_human(&store, "human-eungae", "응애").await?;
    add_human(&store, "human-kicked", "Gone").await?;
    sqlx::query(
        "UPDATE participants SET participant_json=json_set(participant_json, '$.status', 'kicked') WHERE room_id='general' AND participant_id='human-kicked'",
    )
    .execute(&store.pool)
    .await?;
    let assigned = store
        .execute_message_with_turn(
            &principal,
            "people-line",
            "message.send",
            &json!({"content": "@Terra hello"}),
        )
        .await?;
    let view = &assigned.assignments[0].room_view;
    let people = view
        .lines()
        .find(|line| line.starts_with("People: "))
        .unwrap_or_else(|| panic!("missing People line: {view}"));
    assert!(people.contains("Nel Le <@human-nel>"), "{people}");
    assert!(people.contains("응애 <@human-eungae>"), "{people}");
    assert!(!people.contains("human-kicked"), "{people}");
    Ok(())
}

#[tokio::test]
async fn a_newline_in_a_display_name_stays_on_one_line() -> Result<(), Box<dyn std::error::Error>> {
    let (store, _principal, _directory) = fixture().await;
    let human = add_human(&store, "human-line-break", "Line\nBreak").await?;
    let assigned = store
        .execute_message_with_turn(
            &human,
            "line-break",
            "message.send",
            &json!({"content": "@Terra hello"}),
        )
        .await?;
    let view = &assigned.assignments[0].room_view;
    assert!(
        view.contains("Line Break <@human-line-break> [event "),
        "{view}"
    );
    assert!(!view.contains("Line\nBreak"), "{view}");
    let people = view
        .lines()
        .find(|line| line.starts_with("People: "))
        .unwrap_or_else(|| panic!("missing People line: {view}"));
    assert!(
        people.contains("Line Break <@human-line-break>"),
        "{people}"
    );
    Ok(())
}

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

async fn first_turn_started_by(other_asks: bool) -> Result<bool, Box<dyn std::error::Error>> {
    let (store, owner, _directory) = fixture().await;
    sqlx::query(
        "UPDATE participants SET participant_json=json_set(participant_json, '$.owner_id', ?) WHERE room_id='general' AND participant_id=?",
    )
    .bind(&owner.participant_id)
    .bind(AGENT_ID)
    .execute(&store.pool)
    .await?;
    let other = add_human(&store, "human-other", "Other").await?;
    let asker = if other_asks { &other } else { &owner };
    let assigned = store
        .execute_message_with_turn(
            asker,
            "ask",
            "message.send",
            &json!({"content": "@Terra run it"}),
        )
        .await?;
    Ok(assigned.assignments[0].owner_started)
}

/// Full access runs unasked only in a turn that answers the agent's owner.
#[tokio::test]
async fn only_a_turn_answering_the_owner_is_owner_started() -> Result<(), Box<dyn std::error::Error>>
{
    assert!(first_turn_started_by(false).await?);
    assert!(!first_turn_started_by(true).await?);
    Ok(())
}

/// A card stays while an agent in its room uses it; an agent that left loses
/// the reference instead of keeping the card undeletable forever.
#[tokio::test]
async fn a_card_used_only_by_an_agent_that_left_can_be_deleted()
-> Result<(), Box<dyn std::error::Error>> {
    let (store, _owner, _directory) = fixture().await;
    sqlx::query("INSERT INTO persona_assets(persona_id, card_json) VALUES ('guide', '{}')")
        .execute(&store.pool)
        .await?;
    sqlx::query("UPDATE agent_sessions SET session_json = json_set(session_json, '$.persona_card_id', 'guide')")
        .execute(&store.pool)
        .await?;
    assert!(matches!(
        store.delete_persona_asset("guide").await,
        Err(crate::PersistenceError::CommandRejected { code, .. }) if code.as_ref() == "persona_in_use"
    ));
    sqlx::query("UPDATE participants SET participant_json = json_set(participant_json, '$.status', 'kicked') WHERE participant_id = ?")
        .bind(AGENT_ID)
        .execute(&store.pool)
        .await?;
    store.delete_persona_asset("guide").await?;
    let reference: String = sqlx::query_scalar(
        "SELECT json_extract(session_json, '$.persona_card_id') FROM agent_sessions LIMIT 1",
    )
    .fetch_one(&store.pool)
    .await?;
    assert_eq!(reference, "");
    Ok(())
}

/// Discord's "@ on": a reply calls its author only when asked, never yourself.
#[tokio::test]
async fn a_reply_calls_its_author_only_with_reply_mention() -> Result<(), Box<dyn std::error::Error>>
{
    let (store, owner, _directory) = fixture().await;
    let other = add_human(&store, "human-other", "Other").await?;
    let source = store
        .execute_message_with_turn(
            &owner,
            "source",
            "message.send",
            &json!({"content": "first"}),
        )
        .await?;
    let source_id = source.outcome.event.id.clone();
    let called = store
        .execute_message_with_turn(
            &other,
            "called",
            "message.send",
            &json!({"content": "answer", "reply_to_event_id": source_id, "reply_mention": true}),
        )
        .await?;
    assert_eq!(
        called.outcome.event.reply_mention(),
        Some(owner.participant_id.as_str())
    );
    assert!(
        called
            .outcome
            .event
            .addressed_content()
            .starts_with(&format!("<@{}>", owner.participant_id))
    );
    // Backups carry the call, and deleting the reply forgets it.
    crate::backup_json::event(serde_json::to_value(&called.outcome.event)?)?;
    let deleted = agentsassemble_domain::prepare_deleted_message(
        &called.outcome.event,
        agentsassemble_domain::MutableMessageKind::Message,
        chrono::Utc::now(),
    );
    assert_eq!(deleted.reply_mention(), None);
    let quiet = store
        .execute_message_with_turn(
            &other,
            "quiet",
            "message.send",
            &json!({"content": "answer", "reply_to_event_id": source_id}),
        )
        .await?;
    assert_eq!(quiet.outcome.event.reply_mention(), None);
    let own = store
        .execute_message_with_turn(
            &owner,
            "own",
            "message.send",
            &json!({"content": "again", "reply_to_event_id": source_id, "reply_mention": true}),
        )
        .await?;
    assert_eq!(own.outcome.event.reply_mention(), None);
    assert!(
        store
            .execute_message_with_turn(
                &other,
                "no-target",
                "message.send",
                &json!({"content": "x", "reply_mention": true})
            )
            .await
            .is_err()
    );
    Ok(())
}

/// A memory fold is stored once, never overlaps an earlier piece, and goes
/// away with any message it summarized.
#[tokio::test]
async fn a_memory_fold_is_stored_once_and_forgotten_with_its_messages()
-> Result<(), Box<dyn std::error::Error>> {
    let (store, _owner, _directory) = fixture().await;
    let session = stored_session(&store).await;
    let (room, id) = (
        session.public.room_id.clone(),
        session.public.session_id.clone(),
    );
    let mut tx = store.pool.begin().await?;
    // These ranges hold no lobby messages, so each fold read as empty text.
    assert!(crate::agent_memory::store(&mut tx, &room, &id, (5, 20), "", "등대 이야기", 1).await?);
    assert!(!crate::agent_memory::store(&mut tx, &room, &id, (5, 20), "", "again", 2).await?);
    assert!(!crate::agent_memory::store(&mut tx, &room, &id, (15, 30), "", "overlap", 3).await?);
    assert!(crate::agent_memory::store(&mut tx, &room, &id, (21, 30), "", "next", 4).await?);
    crate::agent_memory::forget_message(&mut tx, &room, 25).await?;
    let left: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM agent_memory_pieces")
        .fetch_one(&mut *tx)
        .await?;
    assert_eq!(left, 1);
    tx.commit().await?;
    // The owner's delete empties the piece, and its messages stay unfolded.
    assert!(store.delete_agent_memory(&room, &id, 5).await?);
    assert!(store.agent_memory(&room, &id).await?.is_empty());
    // Someone else's delete inside that range does not bring it back.
    let mut tx = store.pool.begin().await?;
    crate::agent_memory::forget_message(&mut tx, &room, 12).await?;
    tx.commit().await?;
    assert!(
        !store
            .store_agent_memory(&room, &id, (5, 20), "", "again")
            .await?
    );
    Ok(())
}

/// An API agent's turn carries a fold once ten older messages wait, and
/// related memory and pinned messages ride along.
#[tokio::test]
async fn an_api_agent_turn_folds_older_messages_and_carries_related_memory()
-> Result<(), Box<dyn std::error::Error>> {
    let (store, owner, _directory) = fixture().await;
    let mut session = stored_session(&store).await;
    session.public.enabled = false;
    session.public.status = agentsassemble_domain::AgentSessionStatus::Unavailable;
    session.public.runtime_status = agentsassemble_domain::AgentRuntimeStatus::Stopped;
    session.public.provider_session_active = false;
    session.public.runtime_kind = "api".to_owned();
    session.public.execution_harness = "builtin".to_owned();
    save_stored_session(&store, &session).await;
    let mut seqs = Vec::new();
    for index in 0..15 {
        let sent = store
            .execute_message_with_turn(
                &owner,
                &format!("note-{index}"),
                "message.send",
                &json!({"content": format!("등대 기록 {index}")}),
            )
            .await?;
        seqs.push(sent.outcome.event.seq);
    }
    let session = stored_session(&store).await;
    let (room, id) = (
        session.public.room_id.clone(),
        session.public.session_id.clone(),
    );
    let mut tx = store.pool.begin().await?;
    // The newest three are on screen; the twelve before them are due.
    let memory = crate::agent_memory::prepare(&mut tx, &room, &session, seqs[12], "등대")
        .await?
        .ok_or("memory")?;
    let fold = memory.fold.ok_or("fold")?;
    assert_eq!((fold.from_seq, fold.to_seq), (seqs[0], seqs[11]));
    assert!(fold.text.contains("등대 기록 0") && !fold.text.contains("등대 기록 12"));
    // A summary of text that no longer reads the same is refused.
    let stale = format!("{}\n#999 someone: deleted later", fold.text);
    assert!(
        !crate::agent_memory::store(
            &mut tx,
            &room,
            &id,
            (fold.from_seq, fold.to_seq),
            &stale,
            "x",
            1
        )
        .await?
    );
    assert!(
        crate::agent_memory::store(
            &mut tx,
            &room,
            &id,
            (fold.from_seq, fold.to_seq),
            &fold.text,
            "등대지기와 나눈 기록",
            1,
        )
        .await?
    );
    let memory = crate::agent_memory::prepare(&mut tx, &room, &session, seqs[12], "등대지기")
        .await?
        .ok_or("memory")?;
    assert!(
        memory.fold.is_none(),
        "the covered messages are not folded again"
    );
    assert!(memory.context.contains("등대지기와 나눈 기록"));
    Ok(())
}

#[tokio::test]
async fn a_channel_reply_calls_its_author_only_with_reply_mention()
-> Result<(), Box<dyn std::error::Error>> {
    let (store, owner, _directory) = fixture().await;
    let other = add_human(&store, "human-other", "Other").await?;
    let revision = public_settings(&RoomSettings::defaults("General"))?.settings_revision;
    store
        .execute_room_settings_update(
            TrustedPrincipal(&owner),
            "channels",
            &json!({"expected_revision": revision, "channels": [{"id": "c0123456789ab", "name": "notes", "type": "text", "position": 0, "created_at": "2026-09-08T00:00:00Z"}]}),
        )
        .await?;
    let send = |principal, request: &'static str, payload: serde_json::Value| {
        let store = &store;
        async move {
            store
                .execute_channel_message(TrustedPrincipal(principal), request, &payload)
                .await
        }
    };
    let source = send(
        &owner,
        "source",
        json!({"channel_id": "c0123456789ab", "content": "first"}),
    )
    .await?;
    let reply = |content: &str, mention: bool| json!({"channel_id": "c0123456789ab", "content": content, "reply_to_event_id": source.event.id, "reply_mention": mention});
    let called = send(&other, "called", reply("answer", true)).await?;
    assert_eq!(
        called.event.reply_mention(),
        Some(owner.participant_id.as_str())
    );
    let quiet = send(&other, "quiet", reply("answer", false)).await?;
    assert_eq!(quiet.event.reply_mention(), None);
    let own = send(&owner, "own", reply("again", true)).await?;
    assert_eq!(own.event.reply_mention(), None);
    assert!(
        send(
            &other,
            "no-target",
            json!({"channel_id": "c0123456789ab", "content": "x", "reply_mention": true})
        )
        .await
        .is_err()
    );
    Ok(())
}

/// Poll ballots carry no text, so they never fill the fold window.
#[tokio::test]
async fn poll_ballots_do_not_fill_the_memory_fold() -> Result<(), Box<dyn std::error::Error>> {
    let (store, owner, _directory) = fixture().await;
    let mut session = stored_session(&store).await;
    session.public.runtime_kind = "api".to_owned();
    session.public.execution_harness = "builtin".to_owned();
    save_stored_session(&store, &session).await;
    let send = |index: usize| {
        let (store, owner) = (&store, &owner);
        async move {
            store
                .execute_message_with_turn(
                    owner,
                    &format!("note-{index}"),
                    "message.send",
                    &json!({"content": format!("등대 기록 {index}")}),
                )
                .await
        }
    };
    let template = send(0).await?.outcome.event;
    let mut tx = store.pool.begin().await?;
    for _ in 0..70 {
        let mut ballot = template.clone();
        ballot.id = uuid::Uuid::new_v4().to_string();
        ballot.seq = crate::room_event_sequence::next_sequence(&mut tx, &template.room_id).await?;
        ballot.content = Some(String::new());
        ballot.message_kind = Some("vote_cast".to_owned());
        sqlx::query("INSERT INTO room_events(room_id, seq, event_json) VALUES (?, ?, ?)")
            .bind(&ballot.room_id)
            .bind(ballot.seq)
            .bind(serde_json::to_string(&ballot)?)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    let mut last = 0;
    for index in 1..=12 {
        last = send(index).await?.outcome.event.seq;
    }
    let session = stored_session(&store).await;
    let mut tx = store.pool.begin().await?;
    let memory =
        crate::agent_memory::prepare(&mut tx, &session.public.room_id, &session, last + 1, "등대")
            .await?
            .ok_or("memory")?;
    let fold = memory.fold.ok_or("fold")?;
    assert_eq!((fold.from_seq, fold.to_seq), (template.seq, last));
    assert_eq!(fold.text.lines().count(), 13);
    Ok(())
}
