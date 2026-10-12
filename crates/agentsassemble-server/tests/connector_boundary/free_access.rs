use super::{InviteScope, Uuid, fixture, human_invite, json, mcp};
use agentsassemble_domain::{
    AuthenticatedPrincipal, CapabilitySet, ClientKind, LOCAL_OPERATOR_PARTICIPANT_ID,
    LOCAL_OPERATOR_USER_ID, RoomSettings, public_settings,
};
use agentsassemble_persistence::RoomMutationAuthority::TrustedPrincipal;
use agentsassemble_protocol::RoomAction;
use agentsassemble_server::connector::client::RoomConnectorClient;
use rmcp::model::CallToolRequestParams;
use std::time::Duration;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[tokio::test]
async fn free_access_connector_waits_only_for_mentions_and_retains_unread_notification()
-> TestResult {
    for pending in [true, false] {
        let (store, manager) = fixture().await?;
        let principal = AuthenticatedPrincipal {
            principal_id: LOCAL_OPERATOR_USER_ID.into(),
            participant_id: LOCAL_OPERATOR_PARTICIPANT_ID.into(),
            display_name: "Host".into(),
            room_id: "general".into(),
            client_kind: ClientKind::Browser,
            invite_scope: InviteScope::ReadWrite,
            is_operator: true,
            capabilities: CapabilitySet::local_operator(
                ClientKind::Browser,
                InviteScope::ReadWrite,
            ),
        };
        let revision = public_settings(&RoomSettings::defaults("General"))?.settings_revision;
        store
            .execute_room_settings_update(
                TrustedPrincipal(&principal),
                "free",
                &json!({"expected_revision":revision,"conversation_mode":"free_access"}),
            )
            .await?;
        let server = human_invite::start(store.clone()).await;
        let (mut child, reader) = mcp::start_cli().await?;
        let mut links = Vec::new();
        for _ in 0..2 {
            let invite = store
                .create_connector_invite(
                    &manager,
                    Uuid::new_v4(),
                    InviteScope::ReadWrite,
                    chrono::Utc::now(),
                )
                .await?;
            links.push(format!(
                "{}/join?token={}",
                server.base_url, invite.invite_bearer
            ));
        }
        let invitation = json!({"invite_url":links[0],"display_name":"Reader"});
        let joined = mcp::call(&reader, "room_join", invitation.clone()).await;
        assert_eq!(joined["conversation_mode"], "free_access");
        let writer = RoomConnectorClient::new(&links[1], "Writer", None)?;
        writer.join().await?;
        if pending {
            let wait = mcp::call(&reader, "room_wait_next", json!({}));
            let speak = async {
                writer
                    .command(
                        RoomAction::MessageSend,
                        json!({"content":"Ordinary speech"}),
                    )
                    .await?;
                writer
                    .command(
                        RoomAction::MessageSend,
                        json!({"content":"@Nobody unrelated mention"}),
                    )
                    .await?;
                writer
                    .command(
                        RoomAction::MessageSend,
                        json!({"content":"@Reader please read"}),
                    )
                    .await
            };
            let (received, sent) =
                tokio::time::timeout(Duration::from_secs(5), async { tokio::join!(wait, speak) })
                    .await?;
            let sent = sent?;
            assert_eq!(received["messages"][0]["id"], sent["result"]["event"]["id"]);
            assert_eq!(received["messages"].as_array().ok_or("messages")?.len(), 1);
            mcp::call(&reader, "room_read", json!({})).await;
            assert_eq!(
                mcp::call(&reader, "room_status", json!({})).await["unread_mentions"],
                json!([])
            );
        } else {
            let late = writer
                .command(
                    RoomAction::MessageSend,
                    json!({"content":"@Reader while busy"}),
                )
                .await?;
            let event_id = &late["result"]["event"]["id"];
            assert_eq!(
                mcp::call(&reader, "room_join", invitation).await["unread_mentions"][0]["event_id"],
                *event_id
            );
            let immediate = tokio::time::timeout(
                Duration::from_secs(3),
                mcp::call(&reader, "room_wait_next", json!({})),
            )
            .await?;
            assert_eq!(immediate["unread_mentions"][0]["event_id"], *event_id);
            // The next read puts the notification before the room data, then acknowledges returned messages.
            let response = reader
                .call_tool(
                    CallToolRequestParams::new("room_read").with_arguments(serde_json::Map::new()),
                )
                .await?;
            let text = &response
                .content
                .first()
                .and_then(|content| content.as_text())
                .ok_or("text")?
                .text;
            assert!(text.starts_with("{\"unread_mentions\":"));
            let read: serde_json::Value = serde_json::from_str(text)?;
            assert_eq!(read["unread_mentions"][0]["event_id"], *event_id);
            assert_eq!(read["unread_mentions"][0]["display_name"], "Writer");
            assert_eq!(
                mcp::call(&reader, "room_read", json!({})).await["unread_mentions"],
                json!([])
            );
        }
        mcp::call(&reader, "room_leave", json!({})).await;
        writer
            .command(RoomAction::ParticipantLeave, json!({}))
            .await?;
        reader.cancel().await?;
        child.wait().await?;
        server.stop().await;
    }
    Ok(())
}
