use super::*;

#[tokio::test]
async fn connector_creation_uses_exact_purpose_and_recovers_one_invite()
-> Result<(), Box<dyn std::error::Error>> {
    let server = start(true).await;
    let client = reqwest::Client::new();
    let endpoint = format!("{}/api/room-connector/invite", server.base_url);
    let manager = server
        .store
        .authorize_local_room_manager(
            "general",
            LOCAL_OPERATOR_USER_ID,
            LOCAL_OPERATOR_PARTICIPANT_ID,
        )
        .await?;
    let request = json!({"request_id":uuid::Uuid::new_v4().to_string(), "scope":"read_write"});
    let wrong = issue_create(&server, "general").await;
    assert_eq!(
        client
            .post(&endpoint)
            .bearer_auth(wrong)
            .json(&request)
            .send()
            .await?
            .status(),
        401
    );
    let mut results = Vec::new();
    for _ in 0..2 {
        let ticket = server
            .tickets
            .issue_connector_invite_create(manager.clone())
            .await?;
        let response = client
            .post(&endpoint)
            .bearer_auth(&ticket.ticket)
            .json(&request)
            .send()
            .await?;
        assert_eq!(response.status(), 200);
        assert_eq!(response.headers()["cache-control"], "private, no-store");
        results.push(response.json::<Value>().await?);
        assert_eq!(
            client
                .post(&endpoint)
                .bearer_auth(&ticket.ticket)
                .json(&request)
                .send()
                .await?
                .status(),
            401
        );
    }
    assert_eq!(results[0], results[1]);
    assert_eq!(results[0]["room_uid"], manager.room_uid.to_string());
    let join = url::Url::parse(results[0]["join_url"].as_str().ok_or("missing join URL")?)?;
    assert_eq!(join.origin().ascii_serialization(), PUBLIC_ORIGIN);
    let bearer = join
        .query_pairs()
        .find(|(key, _)| key == "token")
        .ok_or("missing token")?
        .1
        .into_owned();
    let admitted = client.post(format!("{}/api/room-connector/join", server.base_url))
        .bearer_auth(bearer).json(&json!({"request_id":uuid::Uuid::new_v4(),"client_secret":base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD,[8;32]),"display_name":"Manager-created connector"})).send().await?;
    assert_eq!(admitted.status(), 200);
    let ticket = server
        .tickets
        .issue_connector_invite_create(manager)
        .await?;
    assert_eq!(
        client
            .post(format!("{}/api/room-invite/create", server.base_url))
            .bearer_auth(ticket.ticket)
            .json(&json!({"meeting_id":"general"}))
            .send()
            .await?
            .status(),
        401
    );
    server.stop().await;
    Ok(())
}
