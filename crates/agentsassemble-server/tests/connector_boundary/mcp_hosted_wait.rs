use super::{InviteScope, Uuid, fixture, human_invite, json, mcp::call};
use rmcp::{ServiceExt, model::CallToolRequestParams, transport::StreamableHttpClientTransport};
use std::time::Duration;

#[tokio::test]
async fn hosted_wait_retains_authenticated_lifetime_and_shutdown_cancels_it()
-> Result<(), Box<dyn std::error::Error>> {
    let (store, manager) = fixture().await?;
    let origin = "https://hosted-wait.example.test";
    let server = human_invite::start_with_public_mcp(
        store.clone(),
        origin,
        "hosted-wait-proxy-fixture-00000001",
    )
    .await;
    let client = ()
        .serve(StreamableHttpClientTransport::from_uri(format!(
            "{}/mcp",
            server.base_url
        )))
        .await?;
    let invite = store
        .create_connector_invite(
            &manager,
            Uuid::new_v4(),
            InviteScope::ReadWrite,
            chrono::Utc::now(),
        )
        .await?;
    let mut args = json!({"invite_url":format!("{origin}/join?token={}", invite.invite_bearer)});
    let prepared = call(&client, "room_join", args.clone()).await;
    args["connection_id"] = prepared["connection_id"].clone();
    call(&client, "room_join", args).await;
    let result = {
        let pending = client.call_tool(
            CallToolRequestParams::new("room_wait_next").with_arguments(
                json!({"connection_id":prepared["connection_id"]})
                    .as_object()
                    .ok_or("arguments")?
                    .clone(),
            ),
        );
        tokio::pin!(pending);
        // Cross the actual production connection lifetime. This is a contract
        // deadline observation, not a sleep used to infer a concurrency interleaving.
        tokio::select! {
            result = &mut pending => panic!("normal room silence ended the hosted wait: {result:?}"),
            () = tokio::time::sleep(Duration::from_secs(31)) => {},
        }
        tokio::time::timeout(Duration::from_secs(5), async {
            let ((), result) = tokio::join!(server.stop(), pending);
            result
        })
        .await?
    };
    assert!(
        result.is_err()
            || result
                .as_ref()
                .is_ok_and(|reply| reply.is_error == Some(true)),
        "shutdown cannot report a successful message"
    );
    client.cancel().await?;
    Ok(())
}
