use super::*;
use crate::support::local_socket;

pub(super) async fn verify(
    server: &human_invite::RunningServer,
    client: &Client,
    connection: &Value,
) -> TestResult {
    let create = call(
        client,
        "create_text_channel",
        json!({"connection_id":connection,"name":"Owner decides","request_id":Uuid::new_v4()}),
    )
    .await;
    let settings = server
        .state()
        .store
        .snapshot("general", 0, 200)
        .await?
        .settings;
    let request = json!({"connection_id":connection,"channel_id":create["target_id"],"expected_settings_revision":agentsassemble_domain::public_settings(&settings)?.settings_revision,"request_id":Uuid::new_v4()});
    let mut owner = local_socket::connect(&server.base_url, server.state(), "general").await;
    assert_eq!(owner.subscribe(0).await["op"], "subscribed");
    assert_eq!(owner.receive_json().await["op"], "snapshot");
    let pending = call(client, "request_delete_text_channel", request.clone()).await;
    loop {
        let frame = owner
            .receive_json_with_timeout(std::time::Duration::from_secs(5))
            .await;
        if frame["events"].as_array().is_some_and(|events| {
            events.iter().any(|event| {
                event["type"] == "delegated_action_requested"
                    && event["delegated_action_request"]["confirmation_id"]
                        == pending["confirmation_id"]
            })
        }) {
            break;
        }
    }
    owner.close().await;
    let mut owner = local_socket::connect(&server.base_url, server.state(), "general").await;
    owner.subscribe(0).await;
    let snapshot = owner.receive_json().await;
    assert_eq!(
        snapshot["delegated_action_requests"][0]["confirmation_id"],
        pending["confirmation_id"]
    );
    assert_eq!(
        snapshot["delegated_action_requests"][0]["target_name"],
        "Owner decides"
    );
    assert!(
        snapshot["delegated_action_requests"][0]["consequences"]
            .as_str()
            .unwrap()
            .contains("message text")
    );
    owner.send_json(&json!({"op":"command","request_id":pending["confirmation_id"],"action":"delegated_action.decide","payload":{"confirmation_id":pending["confirmation_id"],"decision":"decline"}})).await;
    loop {
        let frame = owner
            .receive_json_with_timeout(std::time::Duration::from_secs(5))
            .await;
        if frame["op"] == "ack" {
            assert_eq!(frame["result"]["state"], "declined");
            break;
        }
        assert_ne!(frame["op"], "nack", "{frame}");
    }
    assert_eq!(
        call(client, "request_delete_text_channel", request).await["code"],
        "confirmation_declined"
    );
    assert_eq!(
        server
            .state()
            .store
            .snapshot("general", 0, 200)
            .await?
            .settings,
        settings
    );
    owner.close().await;
    Ok(())
}
