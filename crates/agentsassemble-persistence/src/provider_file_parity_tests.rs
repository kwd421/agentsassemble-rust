use super::*;
use crate::{
    MESSAGE_UPLOAD_PIECE_BYTES, MessageAttachmentReadAuthority, MessageUploadAuthority,
    ProviderAttachmentReadAuthority, ProviderFileAuthority,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;
#[tokio::test]
async fn managed_upload_uses_current_limits_participant_custody_and_streamed_reads() -> TestResult {
    let (store, principal, _directory) = fixture().await;
    let snapshot = store.snapshot("general", 0, 100).await?;
    store.execute_room_settings_update(crate::RoomMutationAuthority::TrustedPrincipal(&principal), "file-limit", &json!({"expected_revision":agentsassemble_domain::public_settings(&snapshot.settings)?.settings_revision,"attachment_file_limit_bytes":50*1024*1024})).await?;
    let mut session = stored_session(&store).await;
    session.public.permission_mode = "workspace_write".into();
    save_stored_session(&store, &session).await;
    let turn = store
        .execute_message_with_turn(
            &principal,
            "file-turn",
            "message.send",
            &json!({"content":"@Terra upload"}),
        )
        .await?;
    let assignment = &turn.assignments[0];
    let start = store
        .authorize_provider_turn_start(
            "general",
            AGENT_ID,
            assignment.turn_generation,
            &assignment.turn_id,
        )
        .await?;
    let file_authority = ProviderFileAuthority {
        room_id: "general".into(),
        session_id: AGENT_ID.into(),
        participant_id: AGENT_ID.into(),
        turn_id: assignment.turn_id.clone(),
        input_up_to_seq: assignment.session.input_up_to_seq,
        turn_generation: assignment.turn_generation,
        execution_id: assignment.execution_id.clone(),
    };
    let custody = MessageUploadAuthority::Provider(Box::new(file_authority.clone()));
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
    store
        .mark_provider_turn_running(&start, "file-result")
        .await?;
    let commit = store
        .complete_agent_turn(
            "general",
            AGENT_ID,
            super::authority(&start, "file-result", None),
            crate::AgentFinalMessage {
                content: "attached",
                target_agent_id: "",
                reply_to_event_id: None,
                attachment_ids: std::slice::from_ref(&completed.attachment_id),
            },
        )
        .await?;
    assert_eq!(
        commit.events[0].extra["attachments"][0]["id"],
        completed.attachment_id
    );
    let next = store
        .execute_message_with_turn(
            &principal,
            "read-turn",
            "message.send",
            &json!({"content":"@Terra read it"}),
        )
        .await?;
    let assignment = &next.assignments[0];
    store
        .authorize_provider_turn_start(
            "general",
            AGENT_ID,
            assignment.turn_generation,
            &assignment.turn_id,
        )
        .await?;
    let authority = ProviderAttachmentReadAuthority {
        room_id: "general",
        session_id: AGENT_ID,
        turn_id: &assignment.turn_id,
        input_up_to_seq: assignment.session.input_up_to_seq,
        turn_generation: assignment.turn_generation,
        execution_id: &assignment.execution_id,
    };
    let file_authority = ProviderFileAuthority {
        turn_id: assignment.turn_id.clone(),
        input_up_to_seq: authority.input_up_to_seq,
        turn_generation: authority.turn_generation,
        execution_id: assignment.execution_id.clone(),
        ..file_authority
    };
    assert!(
        matches!(store.bound_provider_message_attachment(authority, &completed.attachment_id, |_, _| Ok(())).await, Err(PersistenceError::CommandRejected { code, message }) if code=="attachment_too_large" && message.contains(&size.to_string()) && message.contains("10485760"))
    );
    let (reader, ()) = store
        .open_message_attachment_reader(
            MessageAttachmentReadAuthority::Provider(Box::new(file_authority.clone())),
            &completed.attachment_id,
            |_| Ok(()),
        )
        .await?;
    for sequence in 0..size / MESSAGE_UPLOAD_PIECE_BYTES {
        assert_eq!(reader.read_piece(sequence).await?, piece);
    }
    let custody = MessageUploadAuthority::Provider(Box::new(file_authority));
    let snapshot = store.snapshot("general", 0, 100).await?;
    store.execute_room_settings_update(crate::RoomMutationAuthority::TrustedPrincipal(&principal), "quota", &json!({"expected_revision":agentsassemble_domain::public_settings(&snapshot.settings)?.settings_revision,"attachment_storage_limit_bytes":1024*1024*1024})).await?;
    // Existing stored bytes + new reservations share the same quota; no fake profile was created.
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
    let snapshot = store.snapshot("general", 0, 100).await?;
    store.execute_room_settings_update(crate::RoomMutationAuthority::TrustedPrincipal(&principal), "small-room", &json!({"expected_revision":agentsassemble_domain::public_settings(&snapshot.settings)?.settings_revision,"attachment_file_limit_bytes":10*1024*1024})).await?;
    assert!(
        matches!(store.begin_message_upload(&custody, "small-refused", "refused.bin", "application/octet-stream", size).await, Err(PersistenceError::CommandRejected { code, .. }) if code=="attachment_too_large")
    );
    assert_eq!(
        reader.read_piece(0).await?,
        piece,
        "lowered upload policy preserves reads of admitted files, as for humans"
    );
    let mut session = stored_session(&store).await;
    session.public.permission_mode = "meeting_read_only".into();
    save_stored_session(&store, &session).await;
    assert!(
        matches!(store.begin_message_upload(&custody, "conversation-refused", "refused.bin", "application/octet-stream", 1).await, Err(PersistenceError::CommandRejected { code, .. }) if code=="permission_denied")
    );
    // Per-piece authority rejects an ended turn, even for an already-open large reader.
    session.public.active_turn_id.clear();
    save_stored_session(&store, &session).await;
    assert!(reader.read_piece(0).await.is_err());
    Ok(())
}
