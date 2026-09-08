use super::{
    Client, InviteScope, RoomManagerAuthority, SqliteStore, Uuid, Value, fixture, human_invite,
    json,
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[tokio::test]
async fn connector_wait_search_and_vote_read_use_current_public_room_authority() -> TestResult {
    let (store, manager) = fixture().await?;
    let server = human_invite::start(store.clone()).await;
    let client = Client::new();
    let reader = join(
        &client,
        &server.base_url,
        &store,
        &manager,
        InviteScope::ReadOnly,
    )
    .await?;
    let writer = join(
        &client,
        &server.base_url,
        &store,
        &manager,
        InviteScope::ReadWrite,
    )
    .await?;
    let read: Value = client
        .get(format!("{}/api/room-connector/read", server.base_url))
        .bearer_auth(&reader)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert!(read.get("agent_sessions").is_none());
    assert!(read.get("settings").is_none());
    let waiting = client
        .get(format!("{}/api/room-connector/wait", server.base_url))
        .bearer_auth(&reader)
        .query(&[("after_seq", read["last_seq"].to_string())])
        .send();
    let publication = send(
        &client,
        &server.base_url,
        &writer,
        json!({"content":"Wait boundary proof"}),
    );
    let (wait, written) = tokio::time::timeout(std::time::Duration::from_secs(3), async {
        tokio::try_join!(
            async {
                Ok::<_, Box<dyn std::error::Error>>(
                    waiting.await?.error_for_status()?.json::<Value>().await?,
                )
            },
            publication
        )
    })
    .await??;
    assert_eq!(wait["messages"][0]["content"], "Wait boundary proof");
    assert_eq!(wait["last_seq"], written["result"]["event_seq"]);
    verify_search(&client, &server.base_url, &reader).await?;
    let vote = send(
        &client,
        &server.base_url,
        &writer,
        json!({"kind":"vote","vote_question":"Proceed?","vote_options":["Yes","No"]}),
    )
    .await?;
    let vote_id = vote["result"]["event"]["id"]
        .as_str()
        .ok_or("vote id missing")?;
    let summary: Value = client
        .get(format!("{}/api/room-connector/vote", server.base_url))
        .bearer_auth(&reader)
        .query(&[("vote_id", vote_id)])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(summary["vote_id"], vote_id);
    let invalid = client
        .get(format!("{}/api/room-connector/wait", server.base_url))
        .bearer_auth(&reader)
        .query(&[("after_seq", i64::MAX)])
        .send()
        .await?;
    assert_eq!(invalid.status(), 409);
    assert_eq!(
        invalid.json::<Value>().await?["error"]["code"],
        "connector_resync_required"
    );
    server.stop().await;
    Ok(())
}

async fn verify_search(client: &Client, base: &str, bearer: &str) -> TestResult {
    let found: Value = client
        .get(format!("{base}/api/room-connector/search"))
        .bearer_auth(bearer)
        .query(&[("q", "Wait boundary"), ("channel_id", "all")])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(
        found["results"]
            .as_array()
            .ok_or("search results missing")?
            .len(),
        1
    );
    let event = found["results"][0]["event_id"]
        .as_str()
        .ok_or("event id missing")?;
    let channel = found["results"][0]["channel_id"]
        .as_str()
        .ok_or("channel missing")?;
    let context: Value = client
        .get(format!("{base}/api/room-connector/context"))
        .bearer_auth(bearer)
        .query(&[("channel_id", channel), ("event_id", event)])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(context["event_id"], event);
    assert!(
        context["events"]
            .as_array()
            .ok_or("context missing")?
            .iter()
            .any(|row| row["content"] == "Wait boundary proof")
    );
    Ok(())
}

async fn join(
    client: &Client,
    base: &str,
    store: &SqliteStore,
    manager: &RoomManagerAuthority,
    scope: InviteScope,
) -> Result<String, Box<dyn std::error::Error>> {
    let invite = store
        .create_connector_invite(manager, Uuid::new_v4(), scope, chrono::Utc::now())
        .await?;
    let value: Value = client.post(format!("{base}/api/room-connector/join")).bearer_auth(invite.invite_bearer).json(&json!({"request_id":Uuid::new_v4(),"client_secret":URL_SAFE_NO_PAD.encode([8;32]),"display_name":"External conversation"})).send().await?.error_for_status()?.json().await?;
    Ok(value["session_bearer"]
        .as_str()
        .ok_or("session missing")?
        .to_owned())
}

async fn send(
    client: &Client,
    base: &str,
    bearer: &str,
    payload: Value,
) -> Result<Value, Box<dyn std::error::Error>> {
    Ok(client.post(format!("{base}/api/room-connector/command")).bearer_auth(bearer).json(&json!({"request_id":Uuid::new_v4().to_string(),"action":"message.send","payload":payload})).send().await?.error_for_status()?.json().await?)
}
