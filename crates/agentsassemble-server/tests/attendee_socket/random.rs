use super::{Client, TestResult, Value, human_invite::RunningServer, json, local_socket};
use std::time::Duration;
use uuid::Uuid;

pub(super) async fn enable_tabletop(server: &RunningServer) -> TestResult {
    let mut manager = local_socket::connect(&server.base_url, server.state(), "general").await;
    manager.subscribe(0).await;
    let snapshot = manager.receive_json().await;
    manager.send_json(&json!({"op":"command", "request_id":"attendee-tool-mode", "action":"room.settings.update",
        "payload":{"expected_revision":snapshot["room_settings"]["settings_revision"], "tool_mode":"tabletop"}})).await;
    loop {
        let frame = manager
            .receive_json_with_timeout(Duration::from_secs(2))
            .await;
        if frame["op"] == "ack" {
            break;
        }
        assert_ne!(frame["op"], "nack");
    }
    manager.close().await;
    Ok(())
}

pub(super) async fn verify_random(
    server: &RunningServer,
    bearer: &str,
    connection: Uuid,
    turn: &Value,
) -> TestResult {
    let mut events = server.rooms().subscribe("general").await;
    let client = Client::new();
    let endpoint = format!("{}/api/room-attendee/tool/random", server.base_url);
    let request = json!({"request_id":Uuid::new_v4(),
        "turn_generation":turn["assignment"]["authority"]["turn_generation"],
        "execution_id":turn["assignment"]["authority"]["execution_id"],
        "action":"room.random.roll", "payload":{"notation":"2d6+1"}});
    let mut committed = Value::Null;
    for replay in [false, true] {
        let response = client
            .post(&endpoint)
            .bearer_auth(bearer)
            .header("x-attendee-connection-id", connection.to_string())
            .json(&request)
            .send()
            .await?
            .error_for_status()?;
        assert_eq!(response.headers()["cache-control"], "private, no-store");
        let response: Value = response.json().await?;
        assert_eq!(response["resolution"], "committed");
        assert_eq!(response["deduplicated"], replay);
        if replay {
            assert_eq!(response["result"], committed);
        } else {
            committed = response["result"].clone();
        }
    }
    let event = tokio::time::timeout(Duration::from_secs(2), events.recv()).await??;
    assert_eq!(event.extra["operation"], "roll_dice");
    assert_eq!(event.extra["details"]["rolls"], committed["rolls"]);
    assert_eq!(event.extra["details"]["total"], committed["total"]);
    assert!(matches!(
        events.try_recv(),
        Err(tokio::sync::broadcast::error::TryRecvError::Empty)
    ));
    let mut changed = request;
    changed["payload"]["notation"] = json!("1d6");
    assert_eq!(
        client
            .post(&endpoint)
            .bearer_auth(bearer)
            .header("x-attendee-connection-id", connection.to_string())
            .json(&changed)
            .send()
            .await?
            .status(),
        409
    );
    Ok(())
}
