use super::{InviteScope, Uuid, fixture, human_invite, json};
use agentsassemble_protocol::RoomAction;
use agentsassemble_server::connector_client::RoomConnectorClient;
use rmcp::{RoleClient, ServiceExt, model::CallToolRequestParams, service::RunningService};
use serde_json::Value;
use std::{process::Stdio, time::Duration};

#[tokio::test]
async fn packaged_connector_cli_exposes_current_conversation_tools()
-> Result<(), Box<dyn std::error::Error>> {
    let (store, manager) = fixture().await?;
    let server = human_invite::start(store.clone()).await;
    let (mut child, client) = start_cli().await?;
    let tools = client.list_all_tools().await?;
    assert_eq!(tools.len(), 14);
    assert!(tools.iter().any(|tool| tool.name == "room_wait_next"));
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
    let joined = call(
        &client,
        "room_join",
        json!({"invite_url":links[0], "display_name":"CLI conversation"}),
    )
    .await;
    let repeated = call(
        &client,
        "room_join",
        json!({"invite_url":links[0], "display_name":"CLI conversation"}),
    )
    .await;
    assert_eq!(joined["participant_id"], repeated["participant_id"]);
    assert_eq!(joined["connection_id"], repeated["connection_id"]);
    assert_eq!(joined["status"], "joined");
    let blocked = client
        .call_tool(
            CallToolRequestParams::new("room_join").with_arguments(
                json!({"invite_url":links[1]})
                    .as_object()
                    .ok_or("invalid args")?
                    .clone(),
            ),
        )
        .await?;
    assert_eq!(blocked.is_error, Some(true));
    let other = RoomConnectorClient::new(&links[1], "Other AI", None)?;
    other.join().await?;
    let read = call(&client, "room_read", json!({})).await;
    assert!(read["messages"].is_array());
    let wait = call(&client, "room_wait_next", json!({}));
    let contribution = async {
        call(
            &client,
            "room_say",
            json!({"content":"MCP same connection contribution"}),
        )
        .await;
        other
            .command(
                RoomAction::MessageSend,
                json!({"content":"MCP awaited response"}),
            )
            .await
    };
    let (received, sent) = tokio::time::timeout(
        Duration::from_secs(5),
        Box::pin(async { tokio::join!(wait, contribution) }),
    )
    .await?;
    sent?;
    assert_eq!(received["messages"][0]["content"], "MCP awaited response");
    let found = call(
        &client,
        "room_search_messages",
        json!({"query":"same connection"}),
    )
    .await;
    assert_eq!(
        found["results"].as_array().ok_or("missing results")?.len(),
        1
    );
    call(&client, "room_leave", json!({})).await;
    other
        .command(RoomAction::ParticipantLeave, json!({}))
        .await?;
    other.close();
    client.cancel().await?;
    let status = tokio::time::timeout(Duration::from_secs(5), child.wait()).await??;
    assert!(status.success());
    server.stop().await;
    Ok(())
}

pub(super) async fn call(
    client: &RunningService<RoleClient, ()>,
    name: &'static str,
    arguments: Value,
) -> Value {
    let response = client
        .call_tool(
            CallToolRequestParams::new(name).with_arguments(
                arguments
                    .as_object()
                    .unwrap_or_else(|| panic!("object arguments"))
                    .clone(),
            ),
        )
        .await
        .unwrap_or_else(|error| panic!("MCP response: {error}"));
    assert_ne!(response.is_error, Some(true), "{name}: {response:?}");
    let text = response
        .content
        .first()
        .and_then(|content| content.as_text())
        .unwrap_or_else(|| panic!("text result"));
    serde_json::from_str(&text.text).unwrap_or_else(|error| panic!("JSON tool result: {error}"))
}

pub(super) async fn start_cli()
-> Result<(tokio::process::Child, RunningService<RoleClient, ()>), Box<dyn std::error::Error>> {
    let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_assemble"))
        .args(["room", "connector-mcp"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()?;
    let client = ()
        .serve((
            child.stdout.take().ok_or("missing stdout")?,
            child.stdin.take().ok_or("missing stdin")?,
        ))
        .await?;
    Ok((child, client))
}

#[cfg(unix)]
#[tokio::test]
async fn connector_cli_interrupt_exits_with_stdin_still_open()
-> Result<(), Box<dyn std::error::Error>> {
    let (mut child, client) = start_cli().await?;
    client.list_all_tools().await?;
    let pid =
        rustix::process::Pid::from_raw(i32::try_from(child.id().ok_or("missing child PID")?)?)
            .ok_or("invalid PID")?;
    rustix::process::kill_process(pid, rustix::process::Signal::INT)?;
    let result = tokio::time::timeout(Duration::from_secs(3), child.wait()).await;
    if result.is_err() {
        child.kill().await?;
    }
    assert!(result??.success());
    client.cancel().await?;
    Ok(())
}
