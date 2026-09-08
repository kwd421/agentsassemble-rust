use crate::{
    PersistenceError, attendee_ready_tests::report as ready_report,
    attendee_turn_report_tests::assigned_report,
};
use uuid::Uuid;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[tokio::test]
async fn attendee_delivery_recovers_exact_running_input_and_start_acknowledgement() -> TestResult {
    let (store, first, report, now) = assigned_report().await?;
    let original = store
        .deliver_attendee_turn(&first, now)
        .await?
        .ok_or("delivery missing")?;
    assert!(original.resume);
    assert!(original.input_up_to_seq > 0);
    let replacement = store
        .claim_attendee_connection(first.session(), Uuid::new_v4(), now)
        .await?
        .authorization;
    assert!(store.deliver_attendee_turn(&first, now).await.is_err());
    assert!(matches!(
        store.deliver_attendee_turn(&replacement, now).await,
        Err(PersistenceError::CommandRejected {
            code: "bridge_not_ready",
            ..
        })
    ));
    assert!(
        store
            .record_attendee_turn_started(
                &first,
                &original.authority,
                &original.provider_turn_id,
                now
            )
            .await
            .is_err()
    );
    store
        .record_attendee_ready(&replacement, &ready_report(), now)
        .await?;
    let resumed = store
        .deliver_attendee_turn(&replacement, now)
        .await?
        .ok_or("resume missing")?;
    assert!(resumed.resume);
    assert_eq!(resumed.authority, original.authority);
    assert_eq!(resumed.input, original.input);
    assert_eq!(resumed.input_up_to_seq, original.input_up_to_seq);
    assert_eq!(resumed.provider_turn_id, "external-provider-turn");
    store
        .record_attendee_turn_started(
            &replacement,
            &resumed.authority,
            &resumed.provider_turn_id,
            now,
        )
        .await?;
    assert!(
        store
            .record_attendee_turn_started(
                &replacement,
                &resumed.authority,
                "changed-provider-turn",
                now
            )
            .await
            .is_err()
    );
    let mut wrong = resumed.authority.clone();
    wrong.room_id = "other-room".to_owned();
    assert!(matches!(
        store
            .record_attendee_turn_started(&replacement, &wrong, &resumed.provider_turn_id, now)
            .await,
        Err(PersistenceError::CommandRejected {
            code: "permission_denied",
            ..
        })
    ));
    let fields = serde_json::to_value(&resumed)?;
    let object = fields.as_object().ok_or("wire object missing")?;
    assert_eq!(object.len(), 5);
    assert!(object.contains_key("authority") && object.contains_key("input"));
    let published = store
        .record_attendee_turn_report(&replacement, &report, now)
        .await?;
    assert!(!published.outcome.deduplicated);
    assert!(
        store
            .deliver_attendee_turn(&replacement, now)
            .await?
            .is_none()
    );
    Ok(())
}
