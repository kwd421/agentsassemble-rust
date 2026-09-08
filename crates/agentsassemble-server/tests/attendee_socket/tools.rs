use super::{Client, TestResult, Value, json};
use uuid::Uuid;

pub(super) async fn verify_read_tools(
    base: &str,
    bearer: &str,
    old_connection: Uuid,
    current_connection: Uuid,
    turn: &Value,
) -> Result<Value, Box<dyn std::error::Error>> {
    let client = Client::new();
    let endpoint = format!("{base}/api/room-attendee/tool/read");
    let request = json!({
        "turn_generation":turn["assignment"]["authority"]["turn_generation"],
        "execution_id":turn["assignment"]["authority"]["execution_id"],
        "tool":{"kind":"search_messages", "query":"External socket reply", "cursor":""},
    });
    assert!(
        client
            .post(&endpoint)
            .bearer_auth(bearer)
            .header("x-attendee-connection-id", old_connection.to_string())
            .json(&request)
            .send()
            .await?
            .status()
            .is_client_error()
    );
    let mut wrong = request.clone();
    wrong["execution_id"] = json!(Uuid::new_v4());
    assert!(
        client
            .post(&endpoint)
            .bearer_auth(bearer)
            .header("x-attendee-connection-id", current_connection.to_string())
            .json(&wrong)
            .send()
            .await?
            .status()
            .is_client_error()
    );
    let response = client
        .post(&endpoint)
        .bearer_auth(bearer)
        .header("x-attendee-connection-id", current_connection.to_string())
        .json(&request)
        .send()
        .await?
        .error_for_status()?;
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    let page: Value = response.json().await?;
    assert_eq!(page["kind"], "search_messages");
    let hits = page["result"]["results"]
        .as_array()
        .ok_or("search results missing")?;
    assert_eq!(hits.len(), 1);
    let mut context = request.clone();
    context["tool"] = json!({"kind":"message_context", "event_id":hits[0]["event_id"]});
    let response: Value = client
        .post(&endpoint)
        .bearer_auth(bearer)
        .header("x-attendee-connection-id", current_connection.to_string())
        .json(&context)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(response["kind"], "message_context");
    assert_eq!(response["result"]["event_id"], hits[0]["event_id"]);
    let encoded = serde_json::to_string(&response)?;
    assert!(encoded.contains("External socket reply please"));
    for private in [
        bearer,
        "external-runtime",
        "external-owner",
        "external-lease",
    ] {
        assert!(!encoded.contains(private));
    }
    Ok(request)
}

pub(super) async fn reject_finished_read(
    base: &str,
    bearer: &str,
    connection: Uuid,
    request: &Value,
) -> TestResult {
    assert!(
        Client::new()
            .post(format!("{base}/api/room-attendee/tool/read"))
            .bearer_auth(bearer)
            .header("x-attendee-connection-id", connection.to_string())
            .json(request)
            .send()
            .await?
            .status()
            .is_client_error()
    );
    Ok(())
}
