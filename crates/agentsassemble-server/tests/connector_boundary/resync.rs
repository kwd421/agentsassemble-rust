use super::{InviteScope, Uuid, fixture, human_invite, json, mcp};
use agentsassemble_protocol::RoomAction;
use agentsassemble_server::connector_client::RoomConnectorClient;
use rmcp::model::CallToolRequestParams;
use std::time::Duration;

#[tokio::test]
async fn explicit_mcp_read_recovers_a_wait_history_gap() -> Result<(), Box<dyn std::error::Error>> {
    let (store, manager) = fixture().await?;
    let server = human_invite::start(store.clone()).await;
    let invite = store
        .create_connector_invite(
            &manager,
            Uuid::new_v4(),
            InviteScope::ReadWrite,
            chrono::Utc::now(),
        )
        .await?;
    let link = format!("{}/join?token={}", server.base_url, invite.invite_bearer);
    let (mut child, client) = mcp::start_cli().await?;
    mcp::call(&client, "room_join", json!({"invite_url":link})).await;
    let invite = store
        .create_connector_invite(
            &manager,
            Uuid::new_v4(),
            InviteScope::ReadWrite,
            chrono::Utc::now(),
        )
        .await?;
    let link = format!("{}/join?token={}", server.base_url, invite.invite_bearer);
    let writer = RoomConnectorClient::new(&link, "Gap writer", None)?;
    writer.join().await?;
    for index in 0..201 {
        writer
            .command(
                RoomAction::MessageSend,
                json!({"content":format!("Gap message {index}")}),
            )
            .await?;
    }
    for attempt in 0..2 {
        let rejected = client
            .call_tool(
                CallToolRequestParams::new("room_wait_next").with_arguments(serde_json::Map::new()),
            )
            .await?;
        assert_eq!(rejected.is_error, Some(true));
        assert!(
            rejected
                .content
                .iter()
                .filter_map(|item| item.as_text())
                .any(|item| item.text.contains("connector_resync_required"))
        );
        if attempt == 0 {
            mcp::call(&client, "room_read", json!({})).await;
        }
    }
    let snapshot = mcp::call(&client, "room_read", json!({"resync":true})).await;
    assert!(
        snapshot["messages"]
            .as_array()
            .is_some_and(|messages| messages.len() <= 50)
    );
    // Five 200-event reads use the real principal's 1,000-event/10-second budget.
    // Wait for that declared window; this does not coordinate concurrent work.
    tokio::time::sleep(Duration::from_secs(10)).await;
    let sent = writer
        .command(
            RoomAction::MessageSend,
            json!({"content":"After explicit resync"}),
        )
        .await?;
    // A normal read must not consume this new observation either.
    mcp::call(&client, "room_read", json!({})).await;
    let received = tokio::time::timeout(
        Duration::from_secs(3),
        mcp::call(&client, "room_wait_next", json!({})),
    )
    .await?;
    assert_eq!(
        received["messages"]
            .as_array()
            .ok_or("messages missing")?
            .len(),
        1
    );
    assert_eq!(received["messages"][0]["content"], "After explicit resync");
    assert_eq!(received["last_seq"], sent["result"]["event_seq"]);
    mcp::call(&client, "room_leave", json!({})).await;
    writer
        .command(RoomAction::ParticipantLeave, json!({}))
        .await?;
    writer.close();
    client.cancel().await?;
    assert!(
        tokio::time::timeout(Duration::from_secs(5), child.wait())
            .await??
            .success()
    );
    server.stop().await;
    Ok(())
}
