use agentsassemble_domain::{RoomRandomResult, RoomSettings, public_settings};
use serde_json::json;
use uuid::Uuid;

use crate::{
    AttendeeRandomRequest, RoomMutationAuthority, attendee_turn_report_tests::assigned_report,
    human_session_authority_tests::local_operator_principal,
};

#[tokio::test]
async fn external_random_retry_keeps_one_result_across_concurrency_and_turn_completion()
-> Result<(), Box<dyn std::error::Error>> {
    let (store, connection, turn, now) = assigned_report().await?;
    store.execute_room_settings_update(
        RoomMutationAuthority::TrustedPrincipal(&local_operator_principal()), "external-tabletop",
        &json!({"expected_revision":public_settings(&RoomSettings::defaults("General"))?.settings_revision, "tool_mode":"tabletop"}),
    ).await?;
    let request = AttendeeRandomRequest {
        request_id: Uuid::new_v4(),
        turn_generation: turn.turn_generation,
        execution_id: turn.execution_id.clone(),
        action: "room.random.choose".to_owned(),
        payload: json!({"options":["north", "south"], "reason":""}),
    };
    let first = result(0);
    let other = result(1);
    let (one, two) = tokio::join!(
        store.commit_attendee_random(
            connection.session(),
            connection.connection_id(),
            &request,
            &first,
            now
        ),
        store.commit_attendee_random(
            connection.session(),
            connection.connection_id(),
            &request,
            &other,
            now
        ),
    );
    let one = one?;
    let two = two?;
    assert_ne!(one.outcome.deduplicated, two.outcome.deduplicated);
    assert_eq!(one.result, two.result);
    assert_eq!(one.outcome.event.id, two.outcome.event.id);
    let mut changed = request.clone();
    changed.payload = json!({"options":["east", "west"], "reason":""});
    assert!(matches!(
        store
            .commit_attendee_random(
                connection.session(),
                connection.connection_id(),
                &changed,
                &other,
                now
            )
            .await,
        Err(crate::PersistenceError::CommandConflict)
    ));
    assert!(
        store
            .commit_attendee_random(connection.session(), Uuid::new_v4(), &request, &other, now)
            .await
            .is_err()
    );
    store
        .record_attendee_turn_report(&connection, &turn, now)
        .await?;
    let replay = store
        .commit_attendee_random(
            connection.session(),
            connection.connection_id(),
            &request,
            &other,
            now,
        )
        .await?;
    assert!(replay.outcome.deduplicated);
    assert_eq!(replay.result, one.result);
    assert_eq!(replay.outcome.event.id, one.outcome.event.id);
    changed = request;
    changed.request_id = Uuid::new_v4();
    assert!(
        store
            .commit_attendee_random(
                connection.session(),
                connection.connection_id(),
                &changed,
                &other,
                now
            )
            .await
            .is_err()
    );
    let snapshot = store.snapshot("general", 0, 200).await?;
    assert_eq!(
        snapshot
            .events
            .iter()
            .filter(|event| event.id == one.outcome.event.id)
            .count(),
        1
    );
    assert!(!serde_json::to_string(&one.outcome.result)?.contains(&turn.runtime_lease_token));
    Ok(())
}

fn result(index: usize) -> RoomRandomResult {
    let options = vec!["north".to_owned(), "south".to_owned()];
    RoomRandomResult::ChooseRandom {
        choice: options[index].clone(),
        index,
        option_count: 2,
        options,
    }
}
