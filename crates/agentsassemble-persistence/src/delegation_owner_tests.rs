use super::*;
use crate::human_session_authority_tests::{admitted_fixture, session_fingerprint};
use agentsassemble_domain::{InviteScope, Participant, ParticipantStatus};

type TestResult = Result<(), Box<dyn std::error::Error>>;

async fn bind_delegate(store: &SqliteStore, authority: RoomMutationAuthority<'_>) -> TestResult {
    let owner = authority.principal();
    let mut participant = store
        .participant(&owner.room_id, &owner.participant_id)
        .await?;
    participant.participant_id = "delegate".into();
    participant.owner_id = owner.participant_id.clone();
    participant.participant_type = "agent".into();
    participant.role = ParticipantRole::Director;
    let mut tx = store.pool.begin().await?;
    sqlx::query("INSERT INTO participants(room_id,participant_id,participant_json) VALUES(?,?,?)")
        .bind(&participant.room_id)
        .bind(&participant.participant_id)
        .bind(serde_json::to_string(&participant)?)
        .execute(&mut *tx)
        .await?;
    OwnerBinding::from_mutation(&mut tx, authority)
        .await?
        .save(&mut tx, &owner.room_id, "participant", "delegate")
        .await?;
    tx.commit().await?;
    Ok(())
}

async fn changed_participant(store: &SqliteStore, participant: &Participant) -> TestResult {
    let mut tx = store.pool.begin().await?;
    crate::participant_rows::save_participant_exact(
        &mut tx,
        &participant.room_id,
        &participant.participant_id,
        participant,
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

#[tokio::test]
async fn bound_guest_never_borrows_operator_authority_and_live_changes_revoke() -> TestResult {
    let (store, _) = admitted_fixture(InviteScope::ReadWrite).await;
    let fp = session_fingerprint(&store).await;
    let human = store.authorize_human_session(&fp).await?;
    bind_delegate(&store, RoomMutationAuthority::HumanSession(&human)).await?;
    assert!(store.director_owner_can_pin("general", "delegate").await?);
    let mut tx = store.pool.begin().await?;
    let owner = resolve_pin_owner(&mut tx, "general", "delegate").await?;
    assert!(!owner.is_operator);
    assert!(!owner.capabilities.room_manage);
    tx.commit().await?;
    let mut participant = store.participant("general", &owner.participant_id).await?;
    participant.muted = true;
    changed_participant(&store, &participant).await?;
    assert!(!store.director_owner_can_pin("general", "delegate").await?);
    participant.muted = false;
    participant.status = ParticipantStatus::Left;
    changed_participant(&store, &participant).await?;
    assert!(!store.director_owner_can_pin("general", "delegate").await?);
    participant.status = ParticipantStatus::Joined;
    changed_participant(&store, &participant).await?;
    sqlx::query("UPDATE human_room_sessions SET state='ended' WHERE session_fingerprint=?")
        .bind(fp.as_slice())
        .execute(&store.pool)
        .await?;
    assert!(!store.director_owner_can_pin("general", "delegate").await?);
    let (readonly_store, _) = admitted_fixture(InviteScope::ReadOnly).await;
    let readonly = readonly_store
        .authorize_human_session(&session_fingerprint(&readonly_store).await)
        .await?;
    bind_delegate(
        &readonly_store,
        RoomMutationAuthority::HumanSession(&readonly),
    )
    .await?;
    assert!(
        !readonly_store
            .director_owner_can_pin("general", "delegate")
            .await?
    );
    // Even a valid local binding is rejected when it belongs to another person.
    let local = crate::human_session_authority_tests::local_operator_principal();
    let mut tx = store.pool.begin().await?;
    let other =
        OwnerBinding::from_mutation(&mut tx, RoomMutationAuthority::TrustedPrincipal(&local))
            .await?;
    sqlx::query("UPDATE ai_owner_bindings SET binding_json=?")
        .bind(serde_json::to_string(&other)?)
        .execute(&mut *tx)
        .await?;
    assert!(
        matches!(resolve_pin_owner(&mut tx, "general", "delegate").await,
        Err(PersistenceError::CommandRejected { code, .. }) if code == "owner_authority_unavailable")
    );
    tx.commit().await?;
    Ok(())
}

#[tokio::test]
async fn local_binding_needs_no_socket_but_role_kind_and_historical_binding_stay_required()
-> TestResult {
    let (store, _) = admitted_fixture(InviteScope::ReadWrite).await;
    let local = crate::human_session_authority_tests::local_operator_principal();
    bind_delegate(&store, RoomMutationAuthority::TrustedPrincipal(&local)).await?;
    assert!(store.director_owner_can_pin("general", "delegate").await?);
    let mut delegate = store.participant("general", "delegate").await?;
    delegate.role = ParticipantRole::Agent;
    changed_participant(&store, &delegate).await?;
    assert!(!store.director_owner_can_pin("general", "delegate").await?);
    delegate.role = ParticipantRole::Director;
    delegate.participant_type = "human".into();
    changed_participant(&store, &delegate).await?;
    assert!(!store.director_owner_can_pin("general", "delegate").await?);
    delegate.participant_type = "agent".into();
    changed_participant(&store, &delegate).await?;
    sqlx::query("DELETE FROM ai_owner_bindings")
        .execute(&store.pool)
        .await?;
    let mut tx = store.pool.begin().await?;
    assert!(
        matches!(resolve_pin_owner(&mut tx, "general", "delegate").await,
        Err(PersistenceError::CommandRejected { code, .. }) if code == "owner_authority_unavailable")
    );
    Ok(())
}

#[tokio::test]
async fn paired_owner_cannot_be_replaced_by_same_person_local_or_other_device() -> TestResult {
    let (store, now) = admitted_fixture(InviteScope::ReadWrite).await;
    let local = crate::room_user_identity::test_authority(&store).await;
    let manager = RoomManagerAuthority::Local(local);
    for token in [[11; 32], [12; 32]] {
        store
            .create_operator_pairing(&manager, &token, "https://room.example.test", now)
            .await?;
    }
    let paired = store
        .redeem_operator_pairing(&[11; 32], &[21; 32], "https://room.example.test", now)
        .await?;
    let other = store
        .redeem_operator_pairing(&[12; 32], &[22; 32], "https://room.example.test", now)
        .await?;
    bind_delegate(
        &store,
        RoomMutationAuthority::OperatorSession(&paired.authorization),
    )
    .await?;
    assert!(store.director_owner_can_pin("general", "delegate").await?);
    sqlx::query("UPDATE operator_pairings SET revoked=1 WHERE session_fingerprint=?")
        .bind(paired.authorization.session_fingerprint().as_slice())
        .execute(&store.pool)
        .await?;
    assert!(!store.director_owner_can_pin("general", "delegate").await?);
    assert!(
        store
            .revalidate_room_session_authorization(&crate::RoomSessionAuthorization::Operator(
                other.authorization
            ))
            .await
            .is_ok()
    );
    assert!(store.authorize_room_manager(&manager).await.is_ok());
    Ok(())
}
