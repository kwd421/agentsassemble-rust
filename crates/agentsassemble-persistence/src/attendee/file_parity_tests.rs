use super::connection_tests::fixture;
use super::ready_tests::report as ready_report;
use crate::human_session_authority_tests::{local_operator_principal, session_fingerprint};
use crate::{
    AttendeeFileAuthority, AttendeeTurnOutcome, AttendeeTurnReport, MESSAGE_UPLOAD_PIECE_BYTES,
    MessageAttachmentReadAuthority, MessageUploadAuthority, PersistenceError,
    ProviderFileAuthority, RoomMutationAuthority,
};
use serde_json::json;
use uuid::Uuid;

type TestResult = Result<(), Box<dyn std::error::Error>>;

async fn set_permission_mode(
    store: &crate::SqliteStore,
    session_id: &str,
    mode: &str,
) -> TestResult {
    let encoded: String = sqlx::query_scalar(
        "SELECT session_json FROM agent_sessions WHERE room_id = 'general' AND session_id = ?",
    )
    .bind(session_id)
    .fetch_one(&store.pool)
    .await?;
    let mut session: agentsassemble_domain::DurableAgentSession = serde_json::from_str(&encoded)?;
    mode.clone_into(&mut session.public.permission_mode);
    sqlx::query(
        "UPDATE agent_sessions SET session_json = ? WHERE room_id = 'general' AND session_id = ?",
    )
    .bind(serde_json::to_string(&session)?)
    .bind(session_id)
    .execute(&store.pool)
    .await?;
    Ok(())
}

async fn set_file_limit(
    store: &crate::SqliteStore,
    human: &agentsassemble_domain::AuthenticatedPrincipal,
    request_id: &str,
    limit: usize,
) -> TestResult {
    let snapshot = store.snapshot("general", 0, 100).await?;
    store
        .execute_room_settings_update(
            RoomMutationAuthority::TrustedPrincipal(human),
            request_id,
            &json!({"expected_revision":agentsassemble_domain::public_settings(&snapshot.settings)?.settings_revision,"attachment_file_limit_bytes":limit}),
        )
        .await?;
    Ok(())
}

#[tokio::test]
async fn attendee_upload_uses_current_limits_participant_custody_and_streamed_reads() -> TestResult
{
    let (store, session, now) = fixture().await?;
    let human = store
        .authorize_human_session(&session_fingerprint(&store).await)
        .await?;
    let operator = local_operator_principal();
    set_file_limit(&store, &operator, "file-limit-50", 50 * 1024 * 1024).await?;
    let participant_id = session.principal().participant_id.clone();
    let connection = store
        .claim_attendee_connection(&session, Uuid::new_v4(), None, now)
        .await?
        .authorization;
    store
        .record_attendee_ready(&connection, &ready_report(), now)
        .await?;
    store
        .execute_authorized_message_with_turn(
            RoomMutationAuthority::HumanSession(&human),
            &Uuid::new_v4().to_string(),
            "message.send",
            &json!({"content":"Companion, take the file"}),
        )
        .await?;
    let delivery = store
        .deliver_attendee_turn(&connection, now)
        .await?
        .ok_or("turn missing")?;
    let start = &delivery.authority;
    let agent_session_id = start.session_id.clone();
    set_permission_mode(&store, &agent_session_id, "workspace_write").await?;
    let file_authority = AttendeeFileAuthority {
        room_id: "general".into(),
        session_id: agent_session_id.clone(),
        participant_id: participant_id.clone(),
        session_fingerprint: *session.session_fingerprint(),
        connection_id: connection.connection_id(),
        turn_generation: start.turn_generation,
        execution_id: start.execution_id.clone(),
    };
    let custody = MessageUploadAuthority::Attendee(Box::new(file_authority.clone()));
    let size = 30 * 1024 * 1024;
    let transfer = store
        .begin_message_upload(
            &custody,
            "large",
            "../proof.bin",
            "application/octet-stream",
            size,
        )
        .await?;
    let piece = vec![71; MESSAGE_UPLOAD_PIECE_BYTES];
    let mut offset = 0;
    while offset < size {
        offset = store
            .append_message_upload(&custody, &transfer.handle, offset, &piece)
            .await?;
    }
    let completed = store
        .finish_message_upload(&custody, &transfer.handle)
        .await?;
    assert_eq!(completed.attachment.as_ref().unwrap().filename, "proof.bin");
    // Publishing binds the participant-owned pending upload to the room message.
    let report = AttendeeTurnReport {
        request_id: Uuid::new_v4(),
        turn_id: start.turn_id.clone(),
        turn_generation: start.turn_generation,
        execution_id: start.execution_id.clone(),
        start_dispatch_nonce: start.start_dispatch_nonce.clone(),
        runtime_handle_id: start.runtime_handle_id.clone(),
        runtime_owner_id: start.runtime_owner_id.clone(),
        runtime_lease_token: start.runtime_lease_token.clone(),
        provider_turn_id: "external-provider-turn".to_owned(),
        provider_session_id: None,
        outcome: AttendeeTurnOutcome::Message {
            content: "attached".to_owned(),
            target_agent_id: String::new(),
            reply_to_event_id: None,
            attachment_ids: vec![completed.attachment_id.clone()],
        },
    };
    let commit = store
        .record_attendee_turn_report(&connection, &report, now)
        .await?;
    let event = commit.outcome.event;
    assert_eq!(
        event.extra["attachments"][0]["id"],
        json!(completed.attachment_id)
    );
    // Later turns read the bound file through the same streamed reader.
    store
        .execute_authorized_message_with_turn(
            RoomMutationAuthority::HumanSession(&human),
            &Uuid::new_v4().to_string(),
            "message.send",
            &json!({"content":"Companion, read the file"}),
        )
        .await?;
    let second = store
        .deliver_attendee_turn(&connection, now)
        .await?
        .ok_or("second turn missing")?;
    let next = &second.authority;
    let custody = MessageUploadAuthority::Attendee(Box::new(AttendeeFileAuthority {
        room_id: "general".into(),
        session_id: agent_session_id.clone(),
        participant_id: participant_id.clone(),
        session_fingerprint: *session.session_fingerprint(),
        connection_id: connection.connection_id(),
        turn_generation: next.turn_generation,
        execution_id: next.execution_id.clone(),
    }));
    // The pending upload is participant-owned and streams back byte-exact.
    let read_authority = ProviderFileAuthority {
        room_id: "general".into(),
        session_id: agent_session_id.clone(),
        participant_id: participant_id.clone(),
        turn_id: next.turn_id.clone(),
        input_up_to_seq: second.input_up_to_seq,
        turn_generation: next.turn_generation,
        execution_id: next.execution_id.clone(),
    };
    let (reader, ()) = store
        .open_message_attachment_reader(
            MessageAttachmentReadAuthority::Attendee {
                attendee: Box::new(AttendeeFileAuthority {
                    room_id: "general".into(),
                    session_id: agent_session_id.clone(),
                    participant_id: participant_id.clone(),
                    session_fingerprint: *session.session_fingerprint(),
                    connection_id: connection.connection_id(),
                    turn_generation: next.turn_generation,
                    execution_id: next.execution_id.clone(),
                }),
                provider: Box::new(read_authority),
            },
            &completed.attachment_id,
            |_| Ok(()),
        )
        .await?;
    for sequence in 0..size / MESSAGE_UPLOAD_PIECE_BYTES {
        assert_eq!(reader.read_piece(sequence).await?, piece);
    }
    // Existing stored bytes and new reservations share the same quota.
    let snapshot = store.snapshot("general", 0, 100).await?;
    store.execute_room_settings_update(RoomMutationAuthority::TrustedPrincipal(&operator), "quota", &json!({"expected_revision":agentsassemble_domain::public_settings(&snapshot.settings)?.settings_revision,"attachment_storage_limit_bytes":1024*1024*1024})).await?;
    for n in 0..33 {
        store
            .begin_message_upload(
                &custody,
                &format!("reserve-{n}"),
                "reserve.bin",
                "application/octet-stream",
                size,
            )
            .await?;
    }
    assert!(
        matches!(store.begin_message_upload(&custody, "quota-over", "refused.bin", "application/octet-stream", size).await, Err(PersistenceError::CommandRejected { code, .. }) if code=="room_attachment_quota_reached")
    );
    set_file_limit(&store, &operator, "file-limit-10", 10 * 1024 * 1024).await?;
    assert!(
        matches!(store.begin_message_upload(&custody, "small-refused", "refused.bin", "application/octet-stream", size).await, Err(PersistenceError::CommandRejected { code, .. }) if code=="attachment_too_large")
    );
    set_file_limit(&store, &operator, "file-limit-50-again", 50 * 1024 * 1024).await?;
    set_permission_mode(&store, &agent_session_id, "meeting_read_only").await?;
    assert!(
        matches!(store.begin_message_upload(&custody, "conversation-refused", "refused.bin", "application/octet-stream", 1).await, Err(PersistenceError::CommandRejected { code, .. }) if code=="permission_denied")
    );
    set_permission_mode(&store, &agent_session_id, "workspace_write").await?;
    // A replaced connection stops an already-open large reader on the next piece.
    assert_eq!(reader.read_piece(0).await?, piece);
    let replacement = store
        .claim_attendee_connection(&session, Uuid::new_v4(), None, now)
        .await?
        .authorization;
    assert!(reader.read_piece(0).await.is_err());
    let custody = MessageUploadAuthority::Attendee(Box::new(AttendeeFileAuthority {
        room_id: "general".into(),
        session_id: agent_session_id.clone(),
        participant_id: participant_id.clone(),
        session_fingerprint: *session.session_fingerprint(),
        connection_id: replacement.connection_id(),
        turn_generation: next.turn_generation,
        execution_id: next.execution_id.clone(),
    }));
    let (current_reader, ()) = store
        .open_message_attachment_reader(
            MessageAttachmentReadAuthority::Attendee {
                attendee: Box::new(AttendeeFileAuthority {
                    room_id: "general".into(),
                    session_id: agent_session_id.clone(),
                    participant_id: participant_id.clone(),
                    session_fingerprint: *session.session_fingerprint(),
                    connection_id: replacement.connection_id(),
                    turn_generation: next.turn_generation,
                    execution_id: next.execution_id.clone(),
                }),
                provider: Box::new(ProviderFileAuthority {
                    room_id: "general".into(),
                    session_id: agent_session_id.clone(),
                    participant_id: participant_id.clone(),
                    turn_id: next.turn_id.clone(),
                    input_up_to_seq: second.input_up_to_seq,
                    turn_generation: next.turn_generation,
                    execution_id: next.execution_id.clone(),
                }),
            },
            &completed.attachment_id,
            |_| Ok(()),
        )
        .await?;
    assert_eq!(current_reader.read_piece(0).await?, piece);
    // Per-piece authority rejects an ended turn, even for an already-open large reader.
    let encoded: String = sqlx::query_scalar(
        "SELECT session_json FROM agent_sessions WHERE room_id = 'general' AND session_id = ?",
    )
    .bind(&agent_session_id)
    .fetch_one(&store.pool)
    .await?;
    let mut ended: agentsassemble_domain::DurableAgentSession = serde_json::from_str(&encoded)?;
    ended.public.active_turn_id.clear();
    sqlx::query(
        "UPDATE agent_sessions SET session_json = ? WHERE room_id = 'general' AND session_id = ?",
    )
    .bind(serde_json::to_string(&ended)?)
    .bind(&agent_session_id)
    .execute(&store.pool)
    .await?;
    assert!(current_reader.read_piece(0).await.is_err());
    assert!(
        store
            .append_message_upload(&custody, &transfer.handle, 0, &piece)
            .await
            .is_err()
    );
    Ok(())
}
