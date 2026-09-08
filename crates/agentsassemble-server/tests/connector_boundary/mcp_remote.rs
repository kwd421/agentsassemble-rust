use super::{InviteScope, Uuid, fixture, human_invite, json, mcp::call};
use agentsassemble_server::connector_mcp::ConnectorMcp;
use rmcp::{ServiceExt, model::CallToolRequestParams};

#[tokio::test]
async fn remote_registry_requires_exact_handles_and_destinations()
-> Result<(), Box<dyn std::error::Error>> {
    let (store, manager) = fixture().await?;
    let server = human_invite::start(store.clone()).await;
    let connector = ConnectorMcp::new(Some(vec![server.base_url.clone()]))?;
    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    let (client, service) = tokio::try_join!(
        async { ().serve(client_io).await.map_err(|e| e.to_string()) },
        async {
            connector
                .clone()
                .serve(server_io)
                .await
                .map_err(|e| e.to_string())
        }
    )?;
    let denied = client
        .call_tool(
            CallToolRequestParams::new("room_join").with_arguments(
                json!({"invite_url":"https://unapproved.example.test/join?token=test"})
                    .as_object()
                    .ok_or("args")?
                    .clone(),
            ),
        )
        .await?;
    assert_eq!(denied.is_error, Some(true));
    let mut joins = Vec::new();
    for name in ["First conversation", "Second conversation"] {
        let invite = store
            .create_connector_invite(
                &manager,
                Uuid::new_v4(),
                InviteScope::ReadWrite,
                chrono::Utc::now(),
            )
            .await?;
        joins.push(call(&client, "room_join", json!({"invite_url":format!("{}/join?token={}",server.base_url,invite.invite_bearer), "display_name":name})).await);
    }
    assert_ne!(joins[0]["connection_id"], joins[1]["connection_id"]);
    assert_ne!(joins[0]["participant_id"], joins[1]["participant_id"]);
    for arguments in [json!({}), json!({"connection_id":"unknown"})] {
        let denied = client
            .call_tool(
                CallToolRequestParams::new("room_read")
                    .with_arguments(arguments.as_object().ok_or("args")?.clone()),
            )
            .await?;
        assert_eq!(denied.is_error, Some(true));
    }
    for joined in joins {
        call(
            &client,
            "room_read",
            json!({"connection_id":joined["connection_id"]}),
        )
        .await;
        call(
            &client,
            "room_leave",
            json!({"connection_id":joined["connection_id"]}),
        )
        .await;
    }
    client.cancel().await?;
    service.waiting().await?;
    connector.close();
    server.stop().await;
    Ok(())
}
