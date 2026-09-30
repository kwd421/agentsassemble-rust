use super::{InviteScope, Uuid, fixture, human_invite, json, mcp::call, mcp_remote::rejected};
use rmcp::{ServiceExt, transport::StreamableHttpClientTransport};

const ORIGIN: &str = "https://hosted-mcp.example.test";
const PROXY: &str = "hosted-mcp-fixture-proxy-secret-00000001";

#[tokio::test]
async fn app_owned_mcp_uses_current_ingress_and_original_invite_authority()
-> Result<(), Box<dyn std::error::Error>> {
    let (store, manager) = fixture().await?;
    let server = human_invite::start_with_public_mcp(store.clone(), ORIGIN, PROXY).await;
    let endpoint = format!("{}/mcp", server.base_url);
    let first = ().serve(StreamableHttpClientTransport::from_uri(endpoint.clone())).await?;
    let second = ().serve(StreamableHttpClientTransport::from_uri(endpoint.clone())).await?;
    assert_eq!(first.list_all_tools().await?.len(), 17);
    let http = reqwest::Client::new();
    let init = json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"ingress-proof","version":"1"}}});
    for (origin, proxy, expected) in [
        (ORIGIN, PROXY, 200),
        ("https://foreign.example.test", PROXY, 403),
        (ORIGIN, "wrong", 403),
    ] {
        let response = http
            .post(&endpoint)
            .header("host", "hosted-mcp.example.test")
            .header("origin", origin)
            .header("x-forwarded-proto", "https")
            .header("x-agentsassemble-proxy-token", proxy)
            .header("accept", "application/json, text/event-stream")
            .json(&init)
            .send()
            .await?;
        assert_eq!(response.status().as_u16(), expected);
        if expected == 200 {
            assert_eq!(response.headers()["cache-control"], "private, no-store");
            assert!(
                response.json::<serde_json::Value>().await?["result"]["serverInfo"].is_object()
            );
        }
    }
    rejected(
        &first,
        "room_join",
        json!({"invite_url":"https://foreign.example.test/join?token=test"}),
        "room_server_not_allowed",
    )
    .await;
    // Public callers without an issued invitation cannot exhaust the private
    // connection registry merely by inventing syntactically valid credentials.
    for _ in 0..128 {
        rejected(&second, "room_join", json!({"invite_url":format!("{ORIGIN}/join?token={}{}", agentsassemble_persistence::CONNECTOR_INVITE_PREFIX, "A".repeat(43))}), "invite_unavailable").await;
    }
    let invite = store
        .create_connector_invite(
            &manager,
            Uuid::new_v4(),
            InviteScope::ReadWrite,
            chrono::Utc::now(),
        )
        .await?;
    // This public hostname deliberately cannot resolve. The app-owned relay must
    // use its derived listener while preserving the original invitation authority.
    let mut invitation = json!({"invite_url":format!("{ORIGIN}/join?token={}", invite.invite_bearer),"display_name":"Web conversation"});
    let prepared = call(&first, "room_join", invitation.clone()).await;
    assert_eq!(prepared["status"], "connection_prepared");
    assert!(
        store
            .snapshot("general", 0, 200)
            .await?
            .participants
            .iter()
            .all(|p| p.participant_type != "agent")
    );
    invitation["connection_id"] = prepared["connection_id"].clone();
    let joined = call(&first, "room_join", invitation.clone()).await;
    assert_eq!(joined["status"], "joined");
    assert_eq!(
        call(&first, "room_join", invitation).await["participant_id"],
        joined["participant_id"]
    );
    rejected(&second, "room_read", json!({}), "invalid_connection_id").await;
    let mut reused = json!({"invite_url":format!("{ORIGIN}/join?token={}", invite.invite_bearer)});
    reused["connection_id"] =
        call(&second, "room_join", reused.clone()).await["connection_id"].clone();
    rejected(&second, "room_join", reused, "invite_already_used").await;
    let id = joined["connection_id"].clone();
    call(&first, "room_read", json!({"connection_id":id})).await;
    call(
        &first,
        "room_say",
        json!({"connection_id":id,"content":"App-owned public MCP proof"}),
    )
    .await;
    let snapshot = store.snapshot("general", 0, 200).await?;
    assert_eq!(
        snapshot
            .events
            .iter()
            .filter(|e| e.content.as_deref() == Some("App-owned public MCP proof"))
            .count(),
        1
    );
    call(&first, "room_leave", json!({"connection_id":id})).await;
    call(
        &first,
        "room_leave",
        json!({"connection_id":id,"release_receipt":true}),
    )
    .await;
    first.cancel().await?;
    second.cancel().await?;
    tokio::time::timeout(std::time::Duration::from_secs(5), server.stop()).await?;
    Ok(())
}
