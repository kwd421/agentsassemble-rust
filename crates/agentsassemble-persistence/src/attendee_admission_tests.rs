use crate::{
    AttendeeAdmissionRequest, CompanionInviteRequest, PersistenceError,
    human_session_authority_tests::{admitted_fixture, session_fingerprint},
};
use agentsassemble_domain::{ClientKind, InviteScope};
use chrono::Duration;
use sha2::{Digest, Sha256};
use uuid::Uuid;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[tokio::test]
async fn attendee_admission_has_one_provider_bound_external_owner_and_exact_retry() -> TestResult {
    let (store, now) = admitted_fixture(InviteScope::ReadWrite).await;
    let human = store
        .authorize_human_session(&session_fingerprint(&store).await)
        .await?;
    let invite = store
        .create_companion_attendee_invite(
            &crate::RoomSessionAuthorization::Human(human.clone()),
            CompanionInviteRequest {
                request_id: Uuid::new_v4(),
                provider_kind: "codex_live_session",
                display_name: "Invited AI",
            },
            now,
        )
        .await?;
    let fingerprint: [u8; 32] = Sha256::digest(invite.invite_bearer.as_bytes()).into();
    let join = Uuid::new_v4();
    let make = |client, provider_kind, display_name| AttendeeAdmissionRequest {
        invite_fingerprint: &fingerprint,
        client_fingerprint: client,
        request_id: join,
        provider_kind,
        display_name,
    };
    assert!(matches!(
        store
            .admit_attendee(make(&[1; 32], "opencode_server", "Attendee"), now)
            .await,
        Err(PersistenceError::CommandRejected { code, .. }) if matches!(code.as_bytes(), b"provider_mismatch")
    ));
    assert!(
        store
            .snapshot("general", 0, 200)
            .await?
            .agent_sessions
            .is_empty()
    );
    let (left, right) = tokio::join!(
        store.admit_attendee(make(&[1; 32], "codex_live_session", "Attendee"), now),
        store.admit_attendee(make(&[2; 32], "codex_live_session", "Attendee"), now),
    );
    let (winner, client) = match (left, right) {
        (Ok(admitted), Err(_)) => (admitted, [1; 32]),
        (Err(_), Ok(admitted)) => (admitted, [2; 32]),
        _ => panic!("only one attendee admission may win"),
    };
    let retry = store
        .admit_attendee(
            make(&client, "codex_live_session", "Attendee"),
            now + Duration::seconds(1),
        )
        .await?;
    assert!(retry.deduplicated);
    assert_eq!(retry.event.id, winner.event.id);
    assert_eq!(
        retry.authorization.session_fingerprint(),
        winner.authorization.session_fingerprint()
    );
    assert_eq!(
        retry.authorization.expires_at(),
        winner.authorization.expires_at()
    );
    assert!(
        store
            .admit_attendee(make(&client, "codex_live_session", "Changed"), now)
            .await
            .is_err()
    );
    verify_external_custody(&store, &winner.authorization, &human, now).await?;
    assert!(
        store
            .revalidate_attendee_session(&winner.authorization, now + Duration::hours(2))
            .await
            .is_err()
    );
    sqlx::query("UPDATE human_room_sessions SET state='ended' WHERE session_fingerprint=?")
        .bind(human.session_fingerprint().as_slice())
        .execute(&store.pool)
        .await?;
    assert!(
        store
            .revalidate_attendee_session(&winner.authorization, now)
            .await
            .is_err()
    );
    assert!(
        store
            .admit_attendee(make(&client, "codex_live_session", "Attendee"), now)
            .await
            .is_err()
    );
    Ok(())
}

async fn verify_external_custody(
    store: &crate::SqliteStore,
    authorization: &crate::AttendeeSessionAuthorization,
    human: &crate::HumanSessionAuthorization,
    now: chrono::DateTime<chrono::Utc>,
) -> TestResult {
    let principal = authorization.principal();
    assert_eq!(principal.client_kind, ClientKind::AgentBridge);
    assert!(
        !principal.capabilities.message_send
            && !principal.capabilities.room_history
            && !principal.capabilities.room_manage
    );
    assert!(
        store
            .authorize_connector_session(authorization.session_fingerprint(), now)
            .await
            .is_err()
    );
    assert!(
        store
            .authorize_human_session(authorization.session_fingerprint())
            .await
            .is_err()
    );
    let snapshot = store.snapshot("general", 0, 200).await?;
    assert_eq!(snapshot.agent_sessions.len(), 1);
    let public = &snapshot.agent_sessions[0];
    assert!(public.external_owned && !public.enabled && !public.provider_session_active);
    assert_eq!(public.process_ownership, "external");
    assert_eq!(
        snapshot
            .participants
            .iter()
            .find(|p| p.participant_id == principal.participant_id)
            .ok_or("attendee participant missing")?
            .owner_id,
        human.principal().participant_id
    );
    assert!(
        store
            .load_runtime_reconciliation_candidates()
            .await?
            .is_empty()
    );
    let local = crate::room_user_identity::test_authority(store).await;
    let mut tx = store.pool.begin().await?;
    let manager = local.resolve(&mut tx).await?;
    tx.commit().await?;
    let payload = serde_json::json!({"agent_id":principal.participant_id});
    let authority = crate::RoomMutationAuthority::TrustedPrincipal(&manager);
    assert!(matches!(
        store
            .prepare_agent_start(authority, "host-start", &payload)
            .await,
        Err(PersistenceError::CommandRejected { code, .. }) if matches!(code.as_bytes(), b"external_runtime_owned")
    ));
    assert!(matches!(
        store
            .agent_configuration_candidate(authority, &payload)
            .await,
        Err(PersistenceError::CommandRejected { code, .. }) if matches!(code.as_bytes(), b"external_runtime_owned")
    ));
    Ok(())
}
