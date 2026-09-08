use agentsassemble_domain::{
    ProviderRequest, ProviderRequestKind, ProviderRequestPrompt, public_event_for_principal,
};
use chrono::TimeDelta;
use uuid::Uuid;

use crate::{OpenProviderRequest, PersistenceError, attendee_turn_report_tests::assigned_report};

#[tokio::test]
async fn request_open_replays_one_private_event_and_fences_changed_expired_or_replaced_authority()
-> Result<(), Box<dyn std::error::Error>> {
    let (store, connection, turn, now) = assigned_report().await?;
    let request = request_for(&turn);
    let fingerprint = connection.session().session_fingerprint();
    let id = connection.connection_id();
    let (first, retry) = tokio::join!(
        store.open_attendee_provider_request(fingerprint, id, &request, now),
        store.open_attendee_provider_request(fingerprint, id, &request, now),
    );
    let first = first?;
    let retry = retry?;
    assert_ne!(first.deduplicated, retry.deduplicated);
    assert_eq!(first.event.id, retry.event.id);
    assert_eq!(first.expires_at, retry.expires_at);
    let mut viewer = connection.session().principal().clone();
    viewer.principal_id = "unrelated".to_owned();
    viewer.participant_id = "unrelated".to_owned();
    assert_eq!(
        public_event_for_principal(&first.event, &viewer).event_type,
        "event_hidden"
    );
    viewer.participant_id = first.event.extra["owner_id"]
        .as_str()
        .ok_or("owner missing")?
        .to_owned();
    assert_eq!(
        public_event_for_principal(&first.event, &viewer).event_type,
        "provider_request_opened"
    );
    let mut changed = request.clone();
    changed.request.title = "Changed request".to_owned();
    assert!(matches!(
        store
            .open_attendee_provider_request(fingerprint, id, &changed, now)
            .await,
        Err(PersistenceError::CommandConflict)
    ));
    changed.request.provider_request_id = Uuid::new_v4();
    assert!(matches!(
        store
            .open_attendee_provider_request(fingerprint, id, &changed, now)
            .await,
        Err(PersistenceError::CommandRejected { code, .. }) if matches!(code.as_bytes(), b"provider_request_pending")
    ));
    assert!(matches!(
        store
            .open_attendee_provider_request(fingerprint, id, &request, now + TimeDelta::seconds(60))
            .await,
        Err(PersistenceError::CommandRejected { code, .. }) if matches!(code.as_bytes(), b"provider_request_closed")
    ));
    changed = request.clone();
    changed.execution_id = Uuid::new_v4().to_string();
    assert!(
        store
            .open_attendee_provider_request(fingerprint, id, &changed, now)
            .await
            .is_err()
    );
    let principal = connection.session().principal();
    assert!(
        store
            .open_managed_provider_request(
                &principal.room_id,
                &principal.participant_id,
                &request,
                now
            )
            .await
            .is_err()
    );
    store
        .claim_attendee_connection(connection.session(), Uuid::new_v4(), now)
        .await?;
    assert!(matches!(
        store
            .open_attendee_provider_request(fingerprint, id, &request, now)
            .await,
        Err(PersistenceError::CommandRejected { code, .. }) if matches!(code.as_bytes(), b"bridge_connection_replaced")
    ));
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM provider_requests")
        .fetch_one(&store.pool)
        .await?;
    assert_eq!(count, 1);
    Ok(())
}

pub(super) fn request_for(turn: &crate::AttendeeTurnReport) -> OpenProviderRequest {
    OpenProviderRequest {
        turn_generation: turn.turn_generation,
        execution_id: turn.execution_id.clone(),
        request: ProviderRequest {
            provider_request_id: Uuid::new_v4(),
            request_kind: ProviderRequestKind::ExternalAction,
            title: "Complete provider sign-in".to_owned(),
            description: "Continue when ready".to_owned(),
            timeout_seconds: 60,
            prompt: ProviderRequestPrompt::Acknowledge { action_url: None },
        },
    }
}
