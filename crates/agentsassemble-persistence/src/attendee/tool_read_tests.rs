use crate::{
    AttendeeToolRead, AttendeeToolReadRequest, AttendeeToolReadResult, RoomMutationAuthority,
    RoomSessionAuthorization, attendee::connection_tests::fixture,
    attendee::ready_tests::report as ready_report,
    human_session_authority_tests::session_fingerprint,
};
use serde_json::json;
use uuid::Uuid;

#[tokio::test]
async fn attendee_attachment_read_is_bound_to_prestart_input_and_current_connection()
-> Result<(), Box<dyn std::error::Error>> {
    let (store, session, now) = fixture().await?;
    let connection = store
        .claim_attendee_connection(&session, Uuid::new_v4(), None, now)
        .await?
        .authorization;
    store
        .record_attendee_ready(&connection, &ready_report(), now)
        .await?;
    let human = store
        .authorize_human_session(&session_fingerprint(&store).await)
        .await?;
    let attachment = store
        .store_authorized_message_attachment(
            RoomSessionAuthorization::Human(human.clone()).mutation_authority(),
            "input.txt",
            "text/plain",
            b"exact external input".to_vec(),
        )
        .await?;
    let pending = store
        .store_authorized_message_attachment(
            RoomSessionAuthorization::Human(human.clone()).mutation_authority(),
            "private.txt",
            "text/plain",
            b"not published".to_vec(),
        )
        .await?;
    store
        .execute_authorized_message_with_turn(
            RoomMutationAuthority::HumanSession(&human),
            &Uuid::new_v4().to_string(),
            "message.send",
            &json!({"content":"Read the bound input", "attachment_ids":[attachment.id]}),
        )
        .await?;
    let delivery = store
        .deliver_attendee_turn(&connection, now)
        .await?
        .ok_or("turn missing")?;
    let mut request = AttendeeToolReadRequest {
        turn_generation: delivery.authority.turn_generation,
        execution_id: delivery.authority.execution_id.clone(),
        tool: AttendeeToolRead::Attachment {
            attachment_id: attachment.id.clone(),
        },
    };
    let (read, _) = store
        .read_attendee_tool(
            session.session_fingerprint(),
            connection.connection_id(),
            &request,
            now,
            |_, _| Ok(()),
        )
        .await?;
    let AttendeeToolReadResult::Attachment(read) = read else {
        return Err("attachment result missing".into());
    };
    assert_eq!(read.metadata, attachment);
    assert_eq!(read.content, b"exact external input");
    // A valid grandfathered 50 MiB file must fail before content/base64 is loaded.
    sqlx::query("UPDATE room_message_attachments SET size=52428800 WHERE attachment_id=?")
        .bind(&attachment.id)
        .execute(&store.pool)
        .await?;
    sqlx::query("DELETE FROM room_message_attachment_chunks WHERE attachment_id=?")
        .bind(&attachment.id)
        .execute(&store.pool)
        .await?;
    let mut tx = store.pool.begin().await?;
    let piece = vec![0; crate::room_file_content::PIECE_BYTES];
    for sequence in 0..agentsassemble_domain::MAX_ROOM_FILE_BYTES.div_ceil(piece.len()) {
        crate::room_file_content::RoomFileContent::Message
            .append(&mut tx, &attachment.id, sequence, &piece)
            .await?;
    }
    tx.commit().await?;
    sqlx::query("UPDATE room_events SET event_json=json_set(event_json,'$.attachments[0].size',52428800) WHERE room_id='general' AND seq=(SELECT event_seq FROM room_message_attachments WHERE attachment_id=?)")
        .bind(&attachment.id).execute(&store.pool).await?;
    let refusal = store
        .read_attendee_tool(
            session.session_fingerprint(),
            connection.connection_id(),
            &request,
            now,
            |_, _| Ok(()),
        )
        .await;
    let Err(crate::PersistenceError::CommandRejected { code, message }) = refusal else {
        panic!("oversized attendee read must be rejected");
    };
    assert_eq!(code, "attachment_too_large");
    assert!(message.contains("52428800") && message.contains("10485760"));
    request.tool = AttendeeToolRead::Attachment {
        attachment_id: pending.id,
    };
    assert!(
        store
            .read_attendee_tool(
                session.session_fingerprint(),
                connection.connection_id(),
                &request,
                now,
                |_, _| Ok(())
            )
            .await
            .is_err()
    );
    request.tool = AttendeeToolRead::Attachment {
        attachment_id: attachment.id,
    };
    store
        .record_attendee_turn_started(
            &connection,
            &delivery.authority,
            "external-attachment-turn",
            now,
        )
        .await?;
    assert!(
        store
            .read_attendee_tool(
                session.session_fingerprint(),
                connection.connection_id(),
                &request,
                now,
                |_, _| Ok(())
            )
            .await
            .is_err()
    );
    Ok(())
}
