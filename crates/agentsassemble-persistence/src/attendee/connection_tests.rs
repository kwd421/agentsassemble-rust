use crate::{
    AttendeeAdmissionRequest, AttendeeSessionAuthorization, CompanionInviteRequest,
    PersistenceError, SqliteStore,
    human_session_authority_tests::{admitted_fixture, session_fingerprint},
};
use agentsassemble_domain::InviteScope;
use chrono::{DateTime, Utc};
use sha2::{Digest as _, Sha256};
use uuid::Uuid;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[tokio::test]
async fn attendee_connection_replacement_fences_old_reports_and_old_disconnect() -> TestResult {
    let (store, session, now) = fixture().await?;
    let first = store
        .claim_attendee_connection(&session, Uuid::new_v4(), None, now)
        .await?
        .authorization;
    let replacement = store
        .claim_attendee_connection(&session, Uuid::new_v4(), None, now)
        .await?
        .authorization;
    assert!(matches!(
        store.revalidate_attendee_connection(&first, now).await,
        Err(PersistenceError::CommandRejected { code, .. }) if matches!(code.as_bytes(), b"bridge_connection_replaced")
    ));
    assert!(
        store
            .disconnect_attendee_connection(&first, now)
            .await?
            .is_none()
    );
    store
        .revalidate_attendee_connection(&replacement, now)
        .await?;
    // Revoked membership may close its own network lifetime, but cannot submit ordinary reports.
    sqlx::query("UPDATE room_attendee_invites SET revoked=1 WHERE session_fingerprint=?")
        .bind(session.session_fingerprint().as_slice())
        .execute(&store.pool)
        .await?;
    assert!(
        store
            .revalidate_attendee_connection(&replacement, now)
            .await
            .is_err()
    );
    store
        .disconnect_attendee_connection(&replacement, now)
        .await?;
    let state: String =
        sqlx::query_scalar("SELECT state FROM attendee_connections WHERE session_fingerprint=?")
            .bind(session.session_fingerprint().as_slice())
            .fetch_one(&store.pool)
            .await?;
    assert_eq!(state, "disconnected");
    Ok(())
}

#[tokio::test]
async fn startup_ends_old_attendee_network_lifetimes_without_revoking_admission() -> TestResult {
    let (store, session, now) = fixture().await?;
    let first = store
        .claim_attendee_connection(&session, Uuid::new_v4(), None, now)
        .await?
        .authorization;
    store.disconnect_attendees_before_admission().await?;
    assert!(
        store
            .revalidate_attendee_connection(&first, now)
            .await
            .is_err()
    );
    let replacement = store
        .claim_attendee_connection(&session, Uuid::new_v4(), None, now)
        .await?
        .authorization;
    store
        .revalidate_attendee_connection(&replacement, now)
        .await?;
    assert_eq!(
        store
            .snapshot("general", 0, 200)
            .await?
            .agent_sessions
            .len(),
        1
    );
    assert!(
        store
            .claim_attendee_connection(
                &session,
                Uuid::new_v4(),
                None,
                now + chrono::Duration::hours(2)
            )
            .await
            .is_err()
    );
    Ok(())
}

pub(crate) async fn fixture()
-> Result<(SqliteStore, AttendeeSessionAuthorization, DateTime<Utc>), Box<dyn std::error::Error>> {
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
                display_name: "Companion",
            },
            now,
        )
        .await?;
    let fingerprint: [u8; 32] = Sha256::digest(invite.invite_bearer.as_bytes()).into();
    let admitted = store
        .admit_attendee(
            AttendeeAdmissionRequest {
                invite_fingerprint: &fingerprint,
                client_fingerprint: &[1; 32],
                request_id: Uuid::new_v4(),
                provider_kind: "codex_live_session",
                display_name: "Companion",
            },
            now,
        )
        .await?;
    Ok((store, admitted.authorization, now))
}

#[tokio::test]
async fn execution_os_is_only_the_current_ready_connections_display_metadata() -> TestResult {
    use agentsassemble_domain::ExecutionOs;
    let (store, session, now) = fixture().await?;
    let ready = crate::attendee::ready_tests::report();
    for reported in [
        Some(ExecutionOs::Macos),
        Some(ExecutionOs::Windows),
        Some(ExecutionOs::Linux),
        Some(ExecutionOs::Other),
        None,
    ] {
        let connection = store
            .claim_attendee_connection(&session, Uuid::new_v4(), reported, now)
            .await?
            .authorization;
        assert_eq!(
            store.snapshot("general", 0, 200).await?.agent_sessions[0].execution_os,
            None
        );
        let event = store
            .record_attendee_ready(&connection, &ready, now)
            .await?;
        assert_eq!(
            store.snapshot("general", 0, 200).await?.agent_sessions[0].execution_os,
            reported
        );
        let projection = &event.events[0].extra["agent_session"];
        assert_eq!(
            projection.get("execution_os").cloned(),
            reported.map(|os| serde_json::json!(os))
        );
        let replacement = store
            .claim_attendee_connection(&session, Uuid::new_v4(), None, now)
            .await?
            .authorization;
        assert_eq!(
            store.snapshot("general", 0, 200).await?.agent_sessions[0].execution_os,
            None
        );
        assert!(
            store
                .disconnect_attendee_connection(&connection, now)
                .await?
                .is_none()
        );
        store
            .record_attendee_ready(&replacement, &ready, now)
            .await?;
        assert_eq!(
            store.snapshot("general", 0, 200).await?.agent_sessions[0].execution_os,
            None
        );
        let disconnected = store
            .disconnect_attendee_connection(&replacement, now)
            .await?
            .ok_or("disconnect event missing")?;
        assert!(
            disconnected.extra["agent_session"]
                .get("execution_os")
                .is_none()
        );
    }
    Ok(())
}

#[tokio::test]
async fn v84_upgrade_preserves_attendee_custody_and_adds_only_closed_os_metadata() -> TestResult {
    let (store, session, now) = fixture().await?;
    let connection = store
        .claim_attendee_connection(&session, Uuid::new_v4(), None, now)
        .await?
        .authorization;
    sqlx::query("ALTER TABLE attendee_connections DROP COLUMN execution_os")
        .execute(&store.pool)
        .await?;
    sqlx::query("UPDATE runtime_metadata SET value='84' WHERE key='schema_version'")
        .execute(&store.pool)
        .await?;
    crate::schema_version::upgrade_schema(&store.pool).await?;
    crate::schema_version::upgrade_schema(&store.pool).await?;
    store
        .revalidate_attendee_connection(&connection, now)
        .await?;
    assert_eq!(
        store.snapshot("general", 0, 200).await?.agent_sessions[0].execution_os,
        None
    );
    assert!(
        sqlx::query("UPDATE attendee_connections SET execution_os='private-host-name'")
            .execute(&store.pool)
            .await
            .is_err()
    );
    assert!(
        serde_json::from_str::<agentsassemble_domain::ExecutionOs>("\"private-host-name\"")
            .is_err()
    );
    Ok(())
}
