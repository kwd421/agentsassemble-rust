use crate::{
    AttendeeToolRead, AttendeeToolReadRequest, AttendeeToolReadResult, RoomMutationAuthority,
    RoomSessionAuthorization, attendee_connection_tests::fixture,
    attendee_ready_tests::report as ready_report,
    human_session_authority_tests::session_fingerprint,
};
use serde_json::json;
use uuid::Uuid;

#[tokio::test]
async fn attendee_attachment_read_is_bound_to_prestart_input_and_current_connection()
-> Result<(), Box<dyn std::error::Error>> {
    let (store, session, now) = fixture().await?;
    let connection = store
        .claim_attendee_connection(&session, Uuid::new_v4(), now)
        .await?
        .authorization;
    store
        .record_attendee_ready(&connection, &ready_report(), now)
        .await?;
    let human = store
        .authorize_human_session(&session_fingerprint(&store).await)
        .await?;
    let attachment = store
        .store_room_session_message_attachment(
            &RoomSessionAuthorization::Human(human.clone()),
            "input.txt",
            "text/plain",
            b"exact external input".to_vec(),
        )
        .await?;
    let pending = store
        .store_room_session_message_attachment(
            &RoomSessionAuthorization::Human(human.clone()),
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
    let read = store
        .read_attendee_tool(
            session.session_fingerprint(),
            connection.connection_id(),
            &request,
            now,
        )
        .await?;
    let AttendeeToolReadResult::Attachment(read) = read else {
        return Err("attachment result missing".into());
    };
    assert_eq!(read.metadata, attachment);
    assert_eq!(read.content, b"exact external input");
    request.tool = AttendeeToolRead::Attachment {
        attachment_id: pending.id,
    };
    assert!(
        store
            .read_attendee_tool(
                session.session_fingerprint(),
                connection.connection_id(),
                &request,
                now
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
                now
            )
            .await
            .is_err()
    );
    Ok(())
}
