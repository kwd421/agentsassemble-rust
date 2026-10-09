use crate::host_owner_session_tests::{admission, admit_companion, description, setup};
use crate::{
    CentralOwnerSessionRequest, OwnerAdmissionBinding, RoomManagerAuthority,
    RoomSessionAuthorization, ServerOwnerAuthority, SqliteStore,
};
use chrono::{Duration, Utc};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[tokio::test]
async fn companion_removal_rolls_back_with_cursor_and_preserves_native_companion() -> TestResult {
    let OwnerRemovalFixture {
        store,
        binding,
        companion,
        native,
        ..
    } = removal_fixture().await?;
    let independent = admit_companion(&store, &RoomSessionAuthorization::Operator(native)).await?;
    for identity in [companion.principal(), independent.authorization.principal()] {
        sqlx::query("INSERT INTO provider_requests(room_id,request_id,session_id,turn_generation,execution_id,owner_id,request_json,expires_at,state,open_event_id) VALUES ('room',?,?,1,?,'local-operator-user','{}',?,'open',?)")
            .bind(uuid::Uuid::new_v4().to_string()).bind(&identity.participant_id).bind(uuid::Uuid::new_v4().to_string())
            .bind((Utc::now()+Duration::seconds(60)).timestamp_millis()).bind(uuid::Uuid::new_v4().to_string()).execute(&store.pool).await?;
    }
    let key = commit_removal(&store, &binding).await?;
    for _ in 0..3 {
        store.advance_member_removal_authority(&key).await?;
    }
    sqlx::query("CREATE TRIGGER reject_companion_cursor BEFORE UPDATE ON central_member_removals BEGIN SELECT RAISE(ABORT,'injected companion cursor failure'); END").execute(&store.pool).await?;
    assert!(store.advance_member_removal_companions(&key).await.is_err());
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM room_runtime_cleanup")
            .fetch_one(&store.pool)
            .await?,
        0
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM room_attendee_invites WHERE revoked=1")
            .fetch_one(&store.pool)
            .await?,
        0
    );
    sqlx::query("DROP TRIGGER reject_companion_cursor")
        .execute(&store.pool)
        .await?;
    let before: i64 = sqlx::query_scalar("SELECT total_changes()")
        .fetch_one(&store.pool)
        .await?;
    let page = store.advance_member_removal_companions(&key).await?;
    let affected = sqlx::query_scalar::<_, i64>("SELECT total_changes()")
        .fetch_one(&store.pool)
        .await?
        - before;
    assert!(affected <= 100);
    println!("person removal companion metadata_rows={affected}");
    let cleanup = page.cleanup.ok_or("missing exact companion cleanup")?;
    assert_eq!(cleanup.session_id, companion.principal().participant_id);
    assert_eq!(
        page.room_sessions,
        vec![("room".into(), *companion.session_fingerprint())]
    );
    assert_eq!(page.events.len(), 2);
    assert_eq!(page.events[0].event_type, "participant_left");
    assert_eq!(page.next_phase, "companions");
    assert_eq!(page.events[1].event_type, "provider_request_closed");
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT state FROM provider_requests WHERE session_id=?")
            .bind(&companion.principal().participant_id)
            .fetch_one(&store.pool)
            .await?,
        "cancelled"
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT state FROM provider_requests WHERE session_id=?")
            .bind(&independent.authorization.principal().participant_id)
            .fetch_one(&store.pool)
            .await?,
        "open"
    );
    store
        .revalidate_attendee_session(&independent.authorization, Utc::now())
        .await?;
    assert_eq!(
        store
            .advance_member_removal_companions(&key)
            .await?
            .next_phase,
        "member_invites"
    );
    assert_eq!(sqlx::query_scalar::<_,i64>("SELECT COUNT(*) FROM room_events WHERE json_extract(event_json,'$.type')='participant_left'").fetch_one(&store.pool).await?,1);
    // Store pages do not confirm a physical process stop; existing cleanup owner remains pending.
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM room_runtime_cleanup")
            .fetch_one(&store.pool)
            .await?,
        1
    );
    assert_eq!(store.local_operator_profile().await?.display_name, "Owner");
    Ok(())
}

#[tokio::test]
async fn companion_removal_refuses_changed_server_custody_without_partial_effects() -> TestResult {
    let OwnerRemovalFixture {
        store,
        binding,
        companion,
        ..
    } = removal_fixture().await?;
    let key = commit_removal(&store, &binding).await?;
    for _ in 0..3 {
        store.advance_member_removal_authority(&key).await?;
    }
    sqlx::query("UPDATE agent_sessions SET session_json=json_set(session_json,'$.external_owned',json('false'),'$.process_ownership','server') WHERE session_id=?")
        .bind(&companion.principal().participant_id).execute(&store.pool).await?;
    assert!(
        matches!(store.advance_member_removal_companions(&key).await,Err(crate::PersistenceError::CommandUnresolved{code,..}) if code=="account_removal_companion_custody_changed")
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM room_runtime_cleanup")
            .fetch_one(&store.pool)
            .await?,
        0
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM room_attendee_invites WHERE revoked=1")
            .fetch_one(&store.pool)
            .await?,
        0
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT cursor FROM central_member_removals")
            .fetch_one(&store.pool)
            .await?,
        ""
    );
    Ok(())
}

struct OwnerRemovalFixture {
    store: SqliteStore,
    binding: OwnerAdmissionBinding,
    root: crate::OwnerSessionAuthorization,
    session: RoomSessionAuthorization,
    companion: crate::AttendeeSessionAuthorization,
    native_manager: crate::LocalRoomManagerAuthority,
    native: crate::OperatorSessionAuthorization,
}
async fn removal_fixture() -> Result<OwnerRemovalFixture, Box<dyn std::error::Error>> {
    let (store, binding) = setup().await?;
    store.set_registration_epoch(Some("epoch")).await?;
    let root = store
        .create_owner_session(&admission(binding.clone())?, &description()?)
        .await?;
    let owner = ServerOwnerAuthority::CentralSession(Box::new(root.authorization.clone()));
    let owned_room = store
        .create_room_for_local_operator(&uuid::Uuid::new_v4().to_string(), "room", "Room")
        .await?;
    let direct = store
        .create_central_owner_session(
            &owner,
            &CentralOwnerSessionRequest::host_owned(
                "room",
                owned_room.room.room_uid,
                &[17; 32],
                &[42; 32],
                &binding.origin,
                Utc::now(),
            ),
        )
        .await?;
    store
        .create_operator_pairing(
            &RoomManagerAuthority::Operator(Box::new(direct.authorization)),
            &[7; 32],
            &binding.origin,
            Utc::now(),
        )
        .await?;
    let paired = store
        .redeem_operator_pairing(&[7; 32], &[8; 32], &binding.origin, Utc::now())
        .await?;
    let session = RoomSessionAuthorization::Operator(paired.authorization);
    let companion = admit_companion(&store, &session).await?;
    let native_manager = store
        .authorize_local_room_manager(
            "room",
            agentsassemble_domain::LOCAL_OPERATOR_USER_ID,
            agentsassemble_domain::LOCAL_OPERATOR_PARTICIPANT_ID,
        )
        .await?;
    store
        .create_operator_pairing(
            &RoomManagerAuthority::Local(native_manager.clone()),
            &[9; 32],
            &binding.origin,
            Utc::now(),
        )
        .await?;
    let native = store
        .redeem_operator_pairing(&[9; 32], &[10; 32], &binding.origin, Utc::now())
        .await?;
    Ok(OwnerRemovalFixture {
        store,
        binding,
        root: root.authorization,
        session,
        companion: companion.authorization,
        native_manager,
        native: native.authorization,
    })
}
async fn commit_removal(
    store: &SqliteStore,
    binding: &OwnerAdmissionBinding,
) -> Result<crate::MemberRemovalKey, Box<dyn std::error::Error>> {
    let secure = crate::SecureSessionBinding {
        client_key_fingerprint: [11; 32],
        channel_id: "c".repeat(43),
    };
    let principal = crate::MemberRemovalPrincipal::verified(
        "https://central.example".into(),
        binding.person_id.clone(),
        "r".repeat(43),
        "epoch".into(),
        Utc::now() + Duration::seconds(120),
        secure.clone(),
    )?;
    Ok(store
        .begin_member_account_removal(&principal, &secure)
        .await?)
}
#[tokio::test]
async fn person_removal_fences_owner_roots_and_paired_companions_without_local_attribution()
-> TestResult {
    let OwnerRemovalFixture {
        store,
        binding,
        root,
        session,
        companion,
        native,
        ..
    } = removal_fixture().await?;
    let owner = ServerOwnerAuthority::CentralSession(Box::new(root));
    commit_removal(&store, &binding).await?;
    assert!(store.validate_server_owner(&owner).await.is_err());
    assert!(
        store
            .revalidate_room_session_authorization(&session)
            .await
            .is_err()
    );
    assert!(
        store
            .revalidate_attendee_session(&companion, Utc::now())
            .await
            .is_err()
    );
    assert!(
        store
            .create_owner_session(&admission(binding.clone())?, &description()?)
            .await
            .is_err()
    );
    assert!(
        store
            .redeem_operator_pairing(&[7; 32], &[8; 32], &binding.origin, Utc::now())
            .await
            .is_err()
    );
    store
        .revalidate_room_session_authorization(&RoomSessionAuthorization::Operator(native))
        .await?;
    store
        .validate_server_owner(&ServerOwnerAuthority::LocalOperator)
        .await?;
    assert_eq!(store.local_operator_profile().await?.display_name, "Owner");
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM central_identity_bindings")
            .fetch_one(&store.pool)
            .await?,
        0
    );
    Ok(())
}
#[tokio::test]
async fn person_removal_keeps_pending_companion_provenance_through_unrelated_cleanup() -> TestResult
{
    let OwnerRemovalFixture {
        store,
        binding,
        root,
        native_manager,
        ..
    } = removal_fixture().await?;
    let key = commit_removal(&store, &binding).await?;
    let roots = store.advance_member_removal_authority(&key).await?;
    assert_eq!(roots.owner_fingerprints, vec![*root.fingerprint()]);
    assert_eq!(roots.next_phase, "owner_pairings");
    let pairings = store.advance_member_removal_authority(&key).await?;
    assert_eq!(pairings.room_sessions.len(), 2);
    assert_eq!(pairings.next_phase, "human_sessions");
    assert_eq!(
        store
            .advance_member_removal_authority(&key)
            .await?
            .next_phase,
        "companions"
    );
    // Another person's normal admission/cleanup cannot delete pending companion provenance.
    sqlx::query("UPDATE host_owner_sessions SET admission_expires_at=0 WHERE person_id=?")
        .bind(&binding.person_id)
        .execute(&store.pool)
        .await?;
    let mut other = binding.clone();
    other.person_id = "other-person".into();
    other.entry_fingerprint = [99; 32];
    store
        .create_owner_session(&admission(other)?, &description()?)
        .await?;
    store
        .create_operator_pairing(
            &RoomManagerAuthority::Local(native_manager),
            &[98; 32],
            &binding.origin,
            Utc::now(),
        )
        .await?;
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM host_owner_sessions WHERE person_id=?")
            .bind(&binding.person_id)
            .fetch_one(&store.pool)
            .await?,
        1
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM operator_pairings WHERE host_owner_session_fingerprint=?"
        )
        .bind(root.fingerprint().as_slice())
        .fetch_one(&store.pool)
        .await?,
        2
    );
    Ok(())
}
#[tokio::test]
async fn person_removal_pages_checkpoint_with_writes_and_bound_historical_owner_rows()
-> Result<(), Box<dyn std::error::Error>> {
    let (store, binding) = setup().await?;
    store.set_registration_epoch(Some("epoch")).await?;
    let root = store
        .create_owner_session(&admission(binding.clone())?, &description()?)
        .await?;
    let mut other = binding.clone();
    other.person_id = "independent-person".into();
    other.entry_fingerprint = [250; 32];
    store
        .create_owner_session(&admission(other)?, &description()?)
        .await?;
    // Historical ended roots are deliberately larger than one live-root page.
    for number in 2..=97u8 {
        sqlx::query("INSERT INTO host_owner_sessions(fingerprint,session_id,entry_fingerprint,server_id,person_id,device_id,browser_fingerprint,origin,generation,admission_expires_at,created_at,last_connected_at,device_name,browser,os,connected,revoked) SELECT ?,?,?,server_id,person_id,device_id,browser_fingerprint,origin,generation,0,created_at,last_connected_at,device_name,browser,os,0,0 FROM host_owner_sessions WHERE fingerprint=?")
            .bind([number;32].as_slice()).bind(uuid::Uuid::new_v4().to_string()).bind([number;32].as_slice())
            .bind(root.authorization.fingerprint().as_slice()).execute(&store.pool).await?;
    }
    let secure = crate::SecureSessionBinding {
        client_key_fingerprint: [11; 32],
        channel_id: "c".repeat(43),
    };
    let principal = crate::MemberRemovalPrincipal::verified(
        "https://central.example".into(),
        binding.person_id.clone(),
        "r".repeat(43),
        "epoch".into(),
        Utc::now() + Duration::seconds(120),
        secure.clone(),
    )?;
    let key = store
        .begin_member_account_removal(&principal, &secure)
        .await?;
    sqlx::query("CREATE TRIGGER fail_removal_cursor BEFORE UPDATE ON central_member_removals BEGIN SELECT RAISE(ABORT,'injected cursor failure'); END").execute(&store.pool).await?;
    assert!(store.advance_member_removal_authority(&key).await.is_err());
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM host_owner_sessions WHERE revoked=1")
            .fetch_one(&store.pool)
            .await?,
        0
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT cursor FROM central_member_removals")
            .fetch_one(&store.pool)
            .await?,
        ""
    );
    sqlx::query("DROP TRIGGER fail_removal_cursor")
        .execute(&store.pool)
        .await?;
    let mut seen = std::collections::HashSet::new();
    let mut counts = Vec::new();
    loop {
        let page = store.advance_member_removal_authority(&key).await?;
        assert!(page.owner_fingerprints.len() <= 32);
        counts.push(page.owner_fingerprints.len());
        for fingerprint in page.owner_fingerprints {
            assert!(seen.insert(fingerprint));
        }
        if page.next_phase == "owner_pairings" {
            break;
        }
    }
    assert_eq!(counts, vec![32, 32, 32, 1]);
    assert_eq!(seen.len(), 97);
    assert_eq!(sqlx::query_scalar::<_,i64>("SELECT COUNT(*) FROM host_owner_sessions WHERE person_id=? AND revoked=1 AND connected=0 AND device_name='' AND browser='' AND os=''").bind(&binding.person_id).fetch_one(&store.pool).await?,97);
    assert_eq!(sqlx::query_scalar::<_,i64>("SELECT COUNT(*) FROM host_owner_sessions WHERE person_id='independent-person' AND revoked=0 AND connected=1").fetch_one(&store.pool).await?,1);
    sqlx::query("UPDATE central_member_removals SET phase='unknown_phase'")
        .execute(&store.pool)
        .await?;
    assert!(matches!(
        store.advance_member_removal_authority(&key).await,
        Err(crate::PersistenceError::CommandUnresolved { .. })
    ));
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT phase FROM central_member_removals")
            .fetch_one(&store.pool)
            .await?,
        "unknown_phase"
    );
    Ok(())
}
