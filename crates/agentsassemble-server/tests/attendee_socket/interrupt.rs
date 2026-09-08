use super::{
    Client, Duration, InviteScope, Peer, TestResult, URL_SAFE_NO_PAD, Uuid, Value, attendee,
    connect, human_input, human_invite, local_socket, ready,
};
use base64::Engine as _;
use serde_json::json;

#[tokio::test]
async fn external_interrupt_and_mute_deliver_exact_effect_and_publish_http_quiescence() -> TestResult
{
    for (muted, runtime) in [(false, "retained"), (true, "retained"), (false, "gone")] {
        Box::pin(run_case(muted, runtime)).await?;
    }
    Ok(())
}

async fn run_case(muted: bool, runtime: &str) -> TestResult {
    let (store, invite) = attendee::fixture().await?;
    let human_invite = human_invite::persist_invite(
        &store,
        InviteScope::ReadWrite,
        1,
        "interrupt-human",
        "Interrupt Human",
    )
    .await;
    let server = human_invite::start(store.clone()).await;
    let client = Client::new();
    let joined:Value=client.post(format!("{}/api/room-attendee/join",server.base_url)).bearer_auth(&invite.invite_bearer)
        .json(&json!({"request_id":Uuid::new_v4(),"client_secret":URL_SAFE_NO_PAD.encode([12;32]),"provider":"codex","display_name":"Interrupt AI"}))
        .send().await?.error_for_status()?.json().await?;
    let bearer = joined["session_bearer"].as_str().ok_or("bearer missing")?;
    let (mut external, connection_id) = connect(&server.base_url, bearer).await?;
    ready(&mut external).await;
    let mut human = human_input(&server.base_url, human_invite.invite_token()).await?;
    let turn = external
        .receive_json_with_timeout(Duration::from_secs(2))
        .await;
    assert_eq!(turn["type"], "turn");
    let mut manager = local_socket::connect(&server.base_url, server.state(), "general").await;
    manager.subscribe(0).await;
    manager.receive_json().await;
    let command = if muted {
        json!({"op":"command","request_id":"external-mute","action":"participant.mute","payload":{"participant_id":joined["participant_id"],"muted":true}})
    } else {
        json!({"op":"command","request_id":"external-interrupt","action":"agent.interrupt","payload":{"agent_id":joined["participant_id"]}})
    };
    manager.send_json(&command).await;
    let ack = receive_ack(&mut manager).await;
    if !muted {
        assert_eq!(ack["result"]["interrupt_requested"], true);
    }
    let interrupt = external
        .receive_json_with_timeout(Duration::from_secs(2))
        .await;
    assert_eq!(interrupt["type"], "interrupt");
    assert_eq!(
        interrupt["interrupt"]["authority"],
        turn["assignment"]["authority"]
    );
    let endpoint = format!("{}/api/room-attendee/interrupt", server.base_url);
    let report =
        json!({"request_id":Uuid::new_v4(),"interrupted":interrupt["interrupt"],"runtime":runtime});
    let denied = client
        .post(&endpoint)
        .bearer_auth(bearer)
        .header("x-attendee-connection-id", Uuid::new_v4().to_string())
        .json(&report)
        .send()
        .await?;
    assert!(!denied.status().is_success());
    let mut published = server.rooms().subscribe("general").await;
    let event_id = report_retry(&client, &endpoint, bearer, connection_id, &report).await?;
    let event = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let event = published.recv().await?;
            if event.id == event_id {
                return Ok::<_, tokio::sync::broadcast::error::RecvError>(event);
            }
        }
    })
    .await??;
    assert!(!serde_json::to_string(&event)?.contains("external-lease"));
    let snapshot = store.snapshot("general", 0, 200).await?;
    assert!(snapshot.agent_sessions[0].active_turn_id.is_empty());
    assert_eq!(
        snapshot.agent_sessions[0].provider_session_active,
        runtime == "retained"
    );
    assert_eq!(
        snapshot
            .events
            .iter()
            .filter(|event| event.event_type == "turn_finished")
            .count(),
        1
    );
    if runtime == "gone" {
        assert!(tokio::time::timeout(Duration::from_secs(2), external.wait_closed()).await?);
    } else {
        external.close().await;
    }
    manager.close().await;
    human.close().await;
    server.stop().await;
    Ok(())
}

async fn receive_ack(peer: &mut Peer) -> Value {
    loop {
        let frame = peer.receive_json_with_timeout(Duration::from_secs(2)).await;
        if frame["op"] == "ack" {
            return frame;
        }
        assert_ne!(frame["op"], "nack", "command failed: {frame}");
    }
}

async fn report_retry(
    client: &Client,
    endpoint: &str,
    bearer: &str,
    connection_id: Uuid,
    report: &Value,
) -> Result<String, Box<dyn std::error::Error>> {
    let mut event_id = String::new();
    for deduplicated in [false, true] {
        let response = client
            .post(endpoint)
            .bearer_auth(bearer)
            .header("x-attendee-connection-id", connection_id.to_string())
            .json(report)
            .send()
            .await?
            .error_for_status()?;
        assert_eq!(response.headers()["cache-control"], "private, no-store");
        let ack: Value = response.json().await?;
        assert_eq!(ack["deduplicated"], deduplicated);
        let current = ack["event_id"].as_str().ok_or("event missing")?;
        if deduplicated {
            assert_eq!(current, event_id);
        } else {
            current.clone_into(&mut event_id);
        }
    }
    Ok(event_id)
}
