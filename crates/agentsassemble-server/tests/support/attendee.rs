use super::human_invite;
use agentsassemble_domain::{
    FriendDetails, FriendParticipantType, InviteScope, LOCAL_OPERATOR_PARTICIPANT_ID,
    LOCAL_OPERATOR_USER_ID, SaveFriend,
};
use agentsassemble_persistence::RoomManagerAuthority;
use uuid::Uuid;

pub async fn fixture() -> Result<
    (
        agentsassemble_persistence::SqliteStore,
        agentsassemble_persistence::AttendeeInvite,
    ),
    Box<dyn std::error::Error>,
> {
    let (store, _) = human_invite::fixture(InviteScope::ReadWrite).await;
    let manager = RoomManagerAuthority::Local(
        store
            .authorize_local_room_manager(
                "general",
                LOCAL_OPERATOR_USER_ID,
                LOCAL_OPERATOR_PARTICIPANT_ID,
            )
            .await?,
    );
    let friend = store
        .save_friend(&SaveFriend {
            friend_id: Uuid::new_v4(),
            expected_revision: 0,
            details: FriendDetails {
                display_name: "External Codex".to_owned(),
                handle: String::new(),
                participant_type: FriendParticipantType::SubscriptionAi,
                provider_kind: "codex_live_session".to_owned(),
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
    let invite = store
        .create_friend_attendee_invite(
            &manager,
            Uuid::new_v4(),
            friend.friend_id,
            chrono::Utc::now(),
        )
        .await?;
    Ok((store, invite))
}
