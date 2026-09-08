use crate::{
    CompanionInviteRequest, PersistenceError, RoomManagerAuthority,
    human_session_authority_tests::{admitted_fixture, session_fingerprint},
};
use agentsassemble_domain::{FriendDetails, FriendParticipantType, InviteScope, SaveFriend};
use chrono::Duration;
use uuid::Uuid;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[tokio::test]
async fn attendee_friend_receipt_preserves_selected_metadata_without_admitting_or_launching()
-> TestResult {
    let (store, now) = admitted_fixture(InviteScope::ReadWrite).await;
    let authority = crate::room_user_identity::test_authority(&store).await;
    let manager = RoomManagerAuthority::Local(authority);
    let friend = store
        .save_friend(&SaveFriend {
            friend_id: Uuid::new_v4(),
            expected_revision: 0,
            details: FriendDetails {
                display_name: "Remote Codex".to_owned(),
                handle: String::new(),
                participant_type: FriendParticipantType::SubscriptionAi,
                provider_kind: "codex".to_owned(),
                connection_kind: "external".to_owned(),
                agent_id: String::new(),
                source_agent_id: String::new(),
                last_meeting_id: String::new(),
                status: "offline".to_owned(),
                source: "manual".to_owned(),
                last_seen_at: None,
            },
        })
        .await?;
    let request = Uuid::new_v4();
    let first = store
        .create_friend_attendee_invite(&manager, request, friend.friend_id, resolve_provider, now)
        .await?;
    store.delete_friend(friend.friend_id).await?;
    let retry = store
        .create_friend_attendee_invite(
            &manager,
            request,
            friend.friend_id,
            resolve_provider,
            now + Duration::seconds(1),
        )
        .await?;
    assert_eq!(first.invite_id, retry.invite_id);
    let same_bearer = first.invite_bearer == retry.invite_bearer;
    assert!(same_bearer);
    assert_eq!(first.expires_at, retry.expires_at);
    assert_eq!(retry.provider_kind, "codex_live_session");
    assert_eq!(retry.display_name, "Remote Codex");
    assert!(
        store
            .create_friend_attendee_invite(
                &manager,
                Uuid::new_v4(),
                friend.friend_id,
                resolve_provider,
                now
            )
            .await
            .is_err()
    );
    assert!(matches!(
        store
            .create_friend_attendee_invite(&manager, request, Uuid::new_v4(), resolve_provider, now)
            .await,
        Err(PersistenceError::CommandConflict)
    ));
    assert!(
        store
            .create_friend_attendee_invite(
                &manager,
                request,
                friend.friend_id,
                resolve_provider,
                now + Duration::hours(2)
            )
            .await
            .is_err()
    );
    let snapshot = store.snapshot("general", 0, 200).await?;
    assert!(snapshot.agent_sessions.is_empty());
    assert!(
        snapshot
            .participants
            .iter()
            .all(|p| p.participant_type == "human")
    );
    Ok(())
}

#[tokio::test]
async fn attendee_companion_limit_and_creation_replay_remain_under_live_human_authority()
-> TestResult {
    let (store, now) = admitted_fixture(InviteScope::ReadWrite).await;
    let fingerprint = session_fingerprint(&store).await;
    let human = store.authorize_human_session(&fingerprint).await?;
    let make = |request_id| CompanionInviteRequest {
        request_id,
        provider_kind: "codex",
        display_name: "Companion",
    };
    let first_id = Uuid::new_v4();
    let first = store
        .create_companion_attendee_invite(&human, make(first_id), now)
        .await?;
    for _ in 0..6 {
        store
            .create_companion_attendee_invite(&human, make(Uuid::new_v4()), now)
            .await?;
    }
    let (left, right) = tokio::join!(
        store.create_companion_attendee_invite(&human, make(Uuid::new_v4()), now),
        store.create_companion_attendee_invite(&human, make(Uuid::new_v4()), now),
    );
    assert_ne!(left.is_ok(), right.is_ok());
    let ((Err(error), Ok(_)) | (Ok(_), Err(error))) = (left, right) else {
        panic!("one eighth companion must win");
    };
    assert!(matches!(
        error,
        PersistenceError::CommandRejected { code, .. } if matches!(code.as_bytes(), b"companion_limit_reached")
    ));
    let retry = store
        .create_companion_attendee_invite(&human, make(first_id), now + Duration::seconds(1))
        .await?;
    assert_eq!(first.invite_id, retry.invite_id);
    assert_eq!(retry.expires_at, now + Duration::minutes(10));
    sqlx::query("UPDATE human_room_sessions SET state='ended' WHERE session_fingerprint=?")
        .bind(fingerprint.as_slice())
        .execute(&store.pool)
        .await?;
    assert!(
        store
            .create_companion_attendee_invite(&human, make(first_id), now)
            .await
            .is_err()
    );
    let (store, now) = admitted_fixture(InviteScope::ReadOnly).await;
    let human = store
        .authorize_human_session(&session_fingerprint(&store).await)
        .await?;
    assert!(matches!(
        store
            .create_companion_attendee_invite(&human, make(Uuid::new_v4()), now)
            .await,
        Err(PersistenceError::CommandRejected { code, .. }) if matches!(code.as_bytes(), b"permission_denied")
    ));
    Ok(())
}

fn resolve_provider(value: &str) -> Option<&'static str> {
    (value == "codex").then_some("codex_live_session")
}
