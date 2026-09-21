use super::*;
use crate::room_attachment::{ProviderAttachment, ProviderAttachmentReadIngress};
use crate::room_portal::{ProviderRoomToolIngress, ProviderRoomToolResult};
use agentsassemble_domain::{RoomMessageSearchPage, RoomMessageSearchResult};
use base64::Engine as _;

fn input_stream(tool: &str, arguments: &Value) -> String {
    tool_stream("deepseek-chat", tool, "tool_calls").replace(
        &Value::String("{}".to_owned()).to_string(),
        &Value::String(arguments.to_string()).to_string(),
    )
}

async fn delivered_result(
    observation: ProviderRoomObservation,
    tool: &str,
    arguments: Value,
) -> String {
    let (outcome, bodies) = room_turn_with_observation(
        &DEEPSEEK_SPEC,
        "deepseek-chat",
        vec![
            tool_stream("deepseek-chat", "read_discussion", "tool_calls"),
            input_stream(tool, &arguments),
            tool_stream("deepseek-chat", "publish_message", "tool_calls"),
        ],
        None,
        Some(observation),
    )
    .await;
    assert!(
        outcome.is_ok(),
        "valid room result aborted the turn: {outcome:?}"
    );
    assert_eq!(bodies.len(), 3);
    bodies[2]["messages"]
        .as_array()
        .and_then(|messages| {
            messages
                .iter()
                .find(|m| m["role"] == "tool" && m["name"] == tool)
        })
        .and_then(|message| message["content"].as_str())
        .unwrap_or_else(|| panic!("tool result missing from subsequent API request"))
        .to_owned()
}

#[tokio::test]
async fn attachment_results_reach_api_and_preserve_turn_completion() {
    for (content, content_type, is_image, expected) in [
        (b"normal text".to_vec(), "text/plain", false, None),
        (base64::engine::general_purpose::STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+a6N8AAAAASUVORK5CYII=").unwrap_or_else(|error| panic!("PNG fixture: {error}")), "image/png", true, Some("room_tool_media_unsupported")),
        (vec![255, 0, 254], "application/octet-stream", false, Some("room_tool_media_unsupported")),
        (vec![b'a'; 150_000], "text/plain", false, Some("room_tool_result_too_large")),
        ("한".repeat(50_000).into_bytes(), "text/plain", false, Some("room_tool_result_too_large")),
        ("\"\\\n".repeat(50_000).into_bytes(), "text/plain", false, Some("room_tool_result_too_large")),
    ] {
        let id = "ma_00000000000000000000000000000001".to_owned();
        let (ingress, mut commands) = ProviderAttachmentReadIngress::channel(1);
        let attachment = ProviderAttachment {
            id: id.clone(), filename: "controlled.txt".to_owned(),
            content_type: content_type.to_owned(), size: content.len(), is_image, content,
        };
        let owner = tokio::spawn(async move {
            let command = commands.recv().await.unwrap_or_else(|| panic!("missing attachment read"));
            assert_eq!(command.session_id(), "session");
            command.complete(Ok(attachment));
        });
        let mut observation = room_request("session").room_observation.unwrap_or_else(|| panic!("observation"));
        observation.view = format!("#1 Human: inspect {id}");
        observation.attachment_ids = vec![id.clone()];
        observation.attachment_ingress = Some(ingress);
        let result = delivered_result(observation, "read_attachment", json!({"attachment_id":id})).await;
        owner.await.unwrap_or_else(|error| panic!("attachment owner: {error}"));
        if let Some(code) = expected {
            let value: Value = serde_json::from_str(&result).unwrap_or_else(|error| panic!("typed tool failure: {error}"));
            assert_eq!(value["ok"], false);
            assert_eq!(value["error"]["code"], code);
        } else {
            assert!(result.starts_with("normal text\n"));
        }
    }
}

#[tokio::test]
async fn search_over_huge_messages_reaches_api_as_previews_instead_of_failing() {
    let (ingress, mut commands) = ProviderRoomToolIngress::channel(1);
    let owner = tokio::spawn(async move {
        let mut command = commands
            .recv()
            .await
            .unwrap_or_else(|| panic!("missing search"));
        command
            .begin_execution()
            .await
            .unwrap_or_else(|error| panic!("search authority: {error}"));
        command.complete(Ok(ProviderRoomToolResult::SearchMessages(
            RoomMessageSearchPage {
                results: (1..=4)
                    .map(|seq| RoomMessageSearchResult {
                        channel_id: "lobby".to_owned(),
                        event_id: format!("message-{seq}"),
                        participant_id: "human".to_owned(),
                        seq,
                        created_at: "2026-09-15T00:00:00Z".to_owned(),
                        author: "Human".to_owned(),
                        content: "한".repeat(12_000),
                        attachment_filenames: vec![],
                    })
                    .collect(),
                next_cursor: String::new(),
            },
        )));
    });
    let mut observation = room_request("session")
        .room_observation
        .unwrap_or_else(|| panic!("observation"));
    observation.room_tool_ingress = Some(ingress);
    let result = delivered_result(observation, "search_messages", json!({"query":"한"})).await;
    owner
        .await
        .unwrap_or_else(|error| panic!("search owner: {error}"));
    // Four 12,000-character messages used to push the result past the tool-result cap
    // and fail the read. Search results are previews now, so the read succeeds and
    // stays small; the cap itself is still exercised by the attachment cases above.
    assert!(
        result.starts_with("#1 Human [event message-1]: "),
        "{result}"
    );
    assert_eq!(result.matches('…').count(), 4, "{result}");
    assert!(result.len() < 8 * 1024, "{} bytes", result.len());
}
