use std::collections::BTreeMap;

use agentsassemble_domain::{
    ProviderRequestKind, ProviderRequestPrompt, ProviderRequestQuestion, ProviderRequestResolution,
};
use chrono::TimeDelta;
use uuid::Uuid;

use crate::{
    PersistenceError, ProviderRequestDeliveryOutcome, RoomMutationAuthority,
    attendee_turn_report_tests::assigned_report,
    human_session_authority_tests::session_fingerprint, provider_request_tests::request_for,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[tokio::test]
async fn concurrent_secret_answers_claim_once_without_persisting_values() -> TestResult {
    let (store, connection, turn, now) = assigned_report().await?;
    let human = store
        .authorize_human_session(&session_fingerprint(&store).await)
        .await?;
    let authority = RoomMutationAuthority::HumanSession(&human);
    let mut request = request_for(&turn);
    request.request.request_kind = ProviderRequestKind::UserInput;
    request.request.prompt = ProviderRequestPrompt::Answers {
        questions: vec![ProviderRequestQuestion {
            id: "credential".to_owned(),
            header: String::new(),
            question: "Enter test value".to_owned(),
            options: Vec::new(),
            multiple: false,
            is_other: true,
            is_secret: true,
        }],
    };
    let id = request.request.provider_request_id;
    store
        .open_attendee_provider_request(
            connection.session().session_fingerprint(),
            connection.connection_id(),
            &request,
            now,
        )
        .await?;
    let resolution = answer("fixture-secret-answer");
    let mut wrong_owner = human.principal().clone();
    wrong_owner.participant_id = "another-human".to_owned();
    assert!(matches!(
        store
            .resolve_provider_request(
                RoomMutationAuthority::TrustedPrincipal(&wrong_owner),
                id,
                &resolution,
                now
            )
            .await,
        Err(PersistenceError::CommandRejected {
            code: "permission_denied",
            ..
        })
    ));
    let (first, retry) = tokio::join!(
        store.resolve_provider_request(authority, id, &resolution, now),
        store.resolve_provider_request(authority, id, &resolution, now),
    );
    let first = first?;
    let retry = retry?;
    assert_eq!(first.event.id, retry.event.id);
    assert_ne!(first.delivery.is_some(), retry.delivery.is_some());
    assert!(!serde_json::to_string(&first.event)?.contains("fixture-secret-answer"));
    let durable: String =
        sqlx::query_scalar("SELECT resolution_json FROM provider_requests WHERE request_id=?")
            .bind(id.to_string())
            .fetch_one(&store.pool)
            .await?;
    assert!(!durable.contains("fixture-secret-answer"));
    assert!(durable.contains("secret_answered_question_ids"));
    assert!(matches!(
        store
            .resolve_provider_request(authority, id, &answer("changed-secret"), now)
            .await,
        Err(PersistenceError::CommandConflict)
    ));
    let delivery = first
        .delivery
        .or(retry.delivery)
        .ok_or("delivery missing")?;
    assert!(delivery.resolution() == &resolution);
    let closed = store
        .complete_provider_request_delivery(
            &delivery,
            ProviderRequestDeliveryOutcome::Delivered,
            now,
        )
        .await?;
    assert_eq!(closed.extra["state"], "resolved");
    let closed_retry = store
        .complete_provider_request_delivery(
            &delivery,
            ProviderRequestDeliveryOutcome::Delivered,
            now,
        )
        .await?;
    assert_eq!(closed.id, closed_retry.id);
    assert!(
        store
            .resolve_provider_request(authority, id, &resolution, now)
            .await?
            .delivery
            .is_none()
    );
    Ok(())
}

#[tokio::test]
async fn deadline_cancellation_and_replaced_connection_fence_delivery() -> TestResult {
    let (store, connection, turn, now) = assigned_report().await?;
    let human = store
        .authorize_human_session(&session_fingerprint(&store).await)
        .await?;
    let principal = human.principal();
    let authority = RoomMutationAuthority::HumanSession(&human);
    let mut request = request_for(&turn);
    let fingerprint = connection.session().session_fingerprint();
    let connection_id = connection.connection_id();
    let opened = store
        .open_attendee_provider_request(fingerprint, connection_id, &request, now)
        .await?;
    let id = request.request.provider_request_id;
    assert!(
        store
            .expire_provider_request(&principal.room_id, id, now)
            .await?
            .is_none()
    );
    let expired = store
        .expire_provider_request(&principal.room_id, id, opened.expires_at)
        .await?
        .ok_or("expiration missing")?;
    assert_eq!(expired.extra["state"], "expired");
    assert!(
        store
            .resolve_provider_request(authority, id, &ProviderRequestResolution::Acknowledge, now)
            .await
            .is_err()
    );
    request.request.provider_request_id = Uuid::new_v4();
    store
        .open_attendee_provider_request(fingerprint, connection_id, &request, now)
        .await?;
    let id = request.request.provider_request_id;
    let delivery = store
        .resolve_provider_request(authority, id, &ProviderRequestResolution::Acknowledge, now)
        .await?
        .delivery
        .ok_or("delivery missing")?;
    assert!(
        store
            .cancel_provider_execution_requests(
                &principal.room_id,
                &connection.session().principal().participant_id,
                turn.turn_generation,
                &Uuid::new_v4().to_string()
            )
            .await?
            .is_empty()
    );
    let cancelled = store
        .cancel_provider_execution_requests(
            &principal.room_id,
            &connection.session().principal().participant_id,
            turn.turn_generation,
            &turn.execution_id,
        )
        .await?;
    assert_eq!(cancelled.len(), 1);
    assert_eq!(cancelled[0].extra["state"], "cancelled");
    assert!(
        store
            .complete_provider_request_delivery(
                &delivery,
                ProviderRequestDeliveryOutcome::Delivered,
                now
            )
            .await
            .is_err()
    );
    request.request.provider_request_id = Uuid::new_v4();
    store
        .open_attendee_provider_request(fingerprint, connection_id, &request, now)
        .await?;
    store
        .claim_attendee_connection(connection.session(), Uuid::new_v4(), now)
        .await?;
    assert!(matches!(
        store
            .resolve_provider_request(
                authority,
                request.request.provider_request_id,
                &ProviderRequestResolution::Acknowledge,
                now + TimeDelta::seconds(1)
            )
            .await,
        Err(PersistenceError::CommandRejected {
            code: "bridge_connection_replaced",
            ..
        })
    ));
    Ok(())
}

fn answer(value: &str) -> ProviderRequestResolution {
    ProviderRequestResolution::Answers {
        answers: BTreeMap::from([("credential".to_owned(), vec![value.to_owned()])]),
    }
}
