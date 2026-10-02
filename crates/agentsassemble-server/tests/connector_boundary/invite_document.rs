use super::{InviteScope, Uuid, fixture, human_invite, json, mcp::call};
use rmcp::{ServiceExt, transport::StreamableHttpClientTransport};

#[tokio::test]
async fn invitation_document_is_readable_without_js_and_does_not_consume_admission()
-> Result<(), Box<dyn std::error::Error>> {
    let (store, manager) = fixture().await?;
    let server = human_invite::start_with_public_mcp(
        store.clone(),
        "https://document-mcp.example.test",
        "document-mcp-test-proxy-secret-000001",
    )
    .await;
    let invite = store
        .create_connector_invite(
            &manager,
            Uuid::new_v4(),
            InviteScope::ReadWrite,
            chrono::Utc::now(),
        )
        .await?;
    let url = format!("{}/join?token={}", server.base_url, invite.invite_bearer);
    let before = store.snapshot("general", 0, 200).await?;
    let http = reqwest::Client::new();
    for method in [
        reqwest::Method::GET,
        reqwest::Method::HEAD,
        reqwest::Method::GET,
    ] {
        let response = http.request(method.clone(), &url).send().await?;
        assert_eq!(response.status(), 200);
        assert_eq!(response.headers()["cache-control"], "private, no-store");
        assert_eq!(response.headers()["referrer-policy"], "no-referrer");
        assert_eq!(
            response.headers()["x-robots-tag"],
            "noindex, nofollow, noarchive"
        );
        assert_eq!(response.headers()["x-content-type-options"], "nosniff");
        assert!(
            response.headers()["content-type"]
                .to_str()?
                .starts_with("text/html")
        );
        let body = response.text().await?;
        if method == reqwest::Method::HEAD {
            assert!(body.is_empty());
        } else {
            assert!(body.contains("room_join 도구가 있으면 참가"));
            assert!(body.contains("room_join 도구가 없으면 등록 방법"));
            assert!(body.contains("connection_prepared"));
            assert!(body.contains(&format!("{}/mcp", server.base_url)));
            assert!(body.contains(&url));
            assert!(body.contains("codex mcp add"));
            assert!(body.contains("claude mcp add"));
            assert!(body.contains("agy mcp add"));
            assert!(!body.contains("<script"));
            assert!(!body.contains("{{"));
            assert!(!body.contains("session_bearer"));
        }
    }
    let after = store.snapshot("general", 0, 200).await?;
    assert_eq!(before.last_seq, after.last_seq);
    assert_eq!(before.participants.len(), after.participants.len());

    // After URL preview/read/HEAD, the same one-use invite must still admit via MCP.
    let client = ()
        .serve(StreamableHttpClientTransport::from_uri(format!(
            "{}/mcp",
            server.base_url
        )))
        .await?;
    let tools = client.list_all_tools().await?;
    assert_eq!(tools.len(), 17);
    let mut args = json!({"invite_url":url,"display_name":"Link reader"});
    let prepared = call(&client, "room_join", args.clone()).await;
    assert_eq!(prepared["status"], "connection_prepared");
    args["connection_id"] = prepared["connection_id"].clone();
    let joined = call(&client, "room_join", args).await;
    assert_eq!(joined["status"], "joined");
    let id = joined["connection_id"].clone();
    call(
        &client,
        "room_say",
        json!({"connection_id":id,"content":"Read-only invitation proof"}),
    )
    .await;
    assert_eq!(
        store
            .snapshot("general", 0, 200)
            .await?
            .events
            .iter()
            .filter(|event| event.content.as_deref() == Some("Read-only invitation proof"))
            .count(),
        1
    );
    call(&client, "room_leave", json!({"connection_id":id})).await;
    call(
        &client,
        "room_leave",
        json!({"connection_id":id,"release_receipt":true}),
    )
    .await;
    client.cancel().await?;
    server.stop().await;
    Ok(())
}

#[tokio::test]
async fn invitation_document_does_not_interpolate_shell_syntax_from_configured_origins()
-> Result<(), Box<dyn std::error::Error>> {
    let (store, manager) = fixture().await?;
    let proxy = "unsafe-origin-document-test-proxy-secret-0001";
    let server = human_invite::start_with_public_mcp(
        store.clone(),
        "https://commands$(id).example.test",
        proxy,
    )
    .await;
    let invite = store
        .create_connector_invite(
            &manager,
            Uuid::new_v4(),
            InviteScope::ReadOnly,
            chrono::Utc::now(),
        )
        .await?;
    let response = reqwest::Client::new()
        .get(format!(
            "{}/join?token={}",
            server.base_url, invite.invite_bearer
        ))
        .header("host", "commands$(id).example.test")
        .header("x-forwarded-proto", "https")
        .header("x-agentsassemble-proxy-token", proxy)
        .send()
        .await?;
    assert_eq!(response.status(), 400);
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    let body = response.text().await?;
    assert!(!body.contains("$(id)"));
    assert!(!body.contains(&invite.invite_bearer));
    server.stop().await;
    Ok(())
}

#[tokio::test]
async fn invitation_document_uses_trusted_public_origin_and_rejects_ambiguous_input()
-> Result<(), Box<dyn std::error::Error>> {
    const ORIGIN: &str = "https://invitation.example.test";
    const PROXY: &str = "invitation-document-test-proxy-secret-000001";
    let (store, manager) = fixture().await?;
    let server = human_invite::start_with_public_mcp(store.clone(), ORIGIN, PROXY).await;
    let invite = store
        .create_connector_invite(
            &manager,
            Uuid::new_v4(),
            InviteScope::ReadOnly,
            chrono::Utc::now(),
        )
        .await?;
    let query = format!("token={}", invite.invite_bearer);
    let http = reqwest::Client::new();
    let endpoint = format!("{}/join/?{query}", server.base_url);
    let public = http
        .get(&endpoint)
        .header("host", "invitation.example.test")
        .header("x-forwarded-proto", "https")
        .header("x-agentsassemble-proxy-token", PROXY)
        .send()
        .await?;
    assert_eq!(public.status(), 200);
    let body = public.text().await?;
    assert!(body.contains(&format!("{ORIGIN}/mcp")));
    assert!(body.contains(&format!("{ORIGIN}/join?{query}")));
    assert!(!body.contains(&server.base_url));
    assert!(!body.contains(PROXY));
    for (host, proxy) in [
        ("attacker.example.test", PROXY),
        ("invitation.example.test", "wrong"),
    ] {
        let denied = http
            .get(&endpoint)
            .header("host", host)
            .header("x-forwarded-proto", "https")
            .header("x-agentsassemble-proxy-token", proxy)
            .send()
            .await?;
        assert_eq!(denied.status(), 403);
        assert!(!denied.text().await?.contains(&invite.invite_bearer));
    }
    for malformed in [
        format!("{query}&{query}"),
        format!("{query}&redirect=https://attacker.example.test"),
        "token=aaci1.%3Cscript%3Ealert(1)%3C/script%3E".to_owned(),
        format!("token=aaci1.{}", "A".repeat(5000)),
    ] {
        let response = http
            .get(format!("{}/join?{malformed}", server.base_url))
            .send()
            .await?;
        assert_eq!(response.status(), 400);
        assert_eq!(response.headers()["cache-control"], "private, no-store");
        let body = response.text().await?;
        assert!(!body.contains(&invite.invite_bearer));
        assert!(!body.contains("<script>"));
        assert!(!body.contains("attacker.example.test"));
    }
    let before = store.snapshot("general", 0, 200).await?;
    assert!(
        before
            .participants
            .iter()
            .all(|p| p.participant_type != "agent")
    );
    server.stop().await;
    Ok(())
}
