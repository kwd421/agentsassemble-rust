use super::*;
use crate::remote_openai::tool_results::{image_budget, tool_result_text};
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
    assert!(
        bodies[0]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|schema| schema["function"]["name"] == tool)
    );
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
            path: None,
            id: id.clone(), filename: "controlled.txt".to_owned(),
            content_type: content_type.to_owned(), size: content.len(), is_image, content: content.into(),
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
async fn a_vision_model_sees_an_image_attachment_right_after_the_tool_result() {
    const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+a6N8AAAAASUVORK5CYII=";
    for (content, content_type, is_image, image) in [
        (
            base64::engine::general_purpose::STANDARD
                .decode(PNG)
                .unwrap(),
            "image/png",
            true,
            true,
        ),
        (vec![255, 0, 254], "application/octet-stream", false, false),
    ] {
        let id = "ma_00000000000000000000000000000001".to_owned();
        let (ingress, mut commands) = ProviderAttachmentReadIngress::channel(1);
        let attachment = ProviderAttachment {
            path: None,
            id: id.clone(),
            filename: "shot.png".to_owned(),
            content_type: content_type.to_owned(),
            size: content.len(),
            is_image,
            content: content.into(),
        };
        let owner = tokio::spawn(async move {
            let command = commands.recv().await.unwrap();
            command.complete(Ok(attachment));
        });
        let mut observation = room_request("session").room_observation.unwrap();
        observation.view = format!("#1 Human: look at {id}");
        observation.attachment_ids = vec![id.clone()];
        observation.attachment_ingress = Some(ingress);
        let (outcome, bodies) = room_turn_with(
            &DEEPSEEK_SPEC,
            "deepseek-chat",
            vec![
                tool_stream("deepseek-chat", "read_discussion", "tool_calls"),
                input_stream("read_attachment", &json!({"attachment_id": id})),
                tool_stream("deepseek-chat", "publish_message", "tool_calls"),
            ],
            None,
            Some(observation),
            true,
        )
        .await;
        owner.await.unwrap();
        assert!(outcome.is_ok(), "{outcome:?}");
        assert!(
            bodies[0]["messages"][0]["content"]
                .as_str()
                .unwrap()
                .contains("You can see images")
        );
        let messages = bodies[2]["messages"].as_array().unwrap();
        let tool = messages
            .iter()
            .position(|m| m["role"] == "tool" && m["name"] == "read_attachment")
            .unwrap();
        let shown = &messages[tool + 1..];
        if image {
            assert!(
                messages[tool]["content"]
                    .as_str()
                    .unwrap()
                    .contains("after the tool results")
            );
            assert_eq!(shown[0]["role"], "user");
            assert_eq!(
                shown[0]["content"][1]["image_url"]["url"],
                format!("data:image/png;base64,{PNG}")
            );
        } else {
            let value: Value =
                serde_json::from_str(messages[tool]["content"].as_str().unwrap()).unwrap();
            assert_eq!(value["error"]["code"], "room_tool_media_unsupported");
            assert!(shown.iter().all(|m| !m["content"].is_array()));
        }
    }
}

#[tokio::test]
async fn search_over_huge_messages_reaches_api_as_previews_instead_of_failing() {
    let (ingress, mut commands) = ProviderRoomToolIngress::channel(1);
    let owner = tokio::spawn(async move {
        let mut discovery = commands.recv().await.expect("delegation discovery");
        discovery
            .begin_execution()
            .await
            .expect("discovery authority");
        discovery.complete(Ok(ProviderRoomToolResult::DelegatedActions(Vec::new())));
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
                        attachment_ids: vec![],
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
        result.starts_with("#1 Human <@human> [event message-1]: "),
        "{result}"
    );
    assert_eq!(result.matches('…').count(), 4, "{result}");
    assert!(result.len() < 8 * 1024, "{} bytes", result.len());
}

#[tokio::test]
async fn rejected_attachment_reason_reaches_api_and_turn_continues() {
    let id = "ma_00000000000000000000000000000001";
    let (ingress, mut commands) = ProviderAttachmentReadIngress::channel(1);
    let owner = tokio::spawn(async move {
        let command = commands
            .recv()
            .await
            .unwrap_or_else(|| panic!("missing read"));
        command.complete(Err(crate::room_attachment::ProviderAttachmentReadError {
            code: "message_attachment_missing".into(),
            message: "The attachment is unavailable in this room.".to_owned(),
        }));
    });
    let mut observation = room_request("session")
        .room_observation
        .unwrap_or_else(|| panic!("observation"));
    observation.view = format!("#1 Human: inspect {id}");
    observation.attachment_ids = vec![id.to_owned()];
    observation.attachment_ingress = Some(ingress);
    let result =
        delivered_result(observation, "read_attachment", json!({"attachment_id": id})).await;
    owner.await.unwrap_or_else(|error| panic!("owner: {error}"));
    let value: Value =
        serde_json::from_str(&result).unwrap_or_else(|error| panic!("error result: {error}"));
    assert_eq!(value["error"]["code"], "room_tool_rejected");
    assert_eq!(
        value["error"]["message"],
        "The attachment is unavailable in this room."
    );
}

#[tokio::test]
async fn eligible_pin_is_discovered_after_api_launch_and_result_reaches_next_round() {
    let (ingress, mut commands) = ProviderRoomToolIngress::channel(1);
    let owner = tokio::spawn(async move {
        let mut discovery = commands.recv().await.expect("discovery");
        discovery
            .begin_execution()
            .await
            .expect("discovery authority");
        discovery.complete(Ok(ProviderRoomToolResult::DelegatedActions(vec![
            "set_message_pin".into(),
        ])));
        let mut pin = commands.recv().await.expect("pin");
        pin.begin_execution().await.expect("pin authority");
        pin.complete(Ok(ProviderRoomToolResult::MessagePin(
            json!({"status":"succeeded"}),
        )));
    });
    let mut observation = room_request("session")
        .room_observation
        .expect("observation");
    observation.room_tool_ingress = Some(ingress);
    let result = delivered_result(observation, "set_message_pin", json!({"channel_id":"lobby","message_id":"message-1","pinned":true,"request_id":uuid::Uuid::new_v4()})).await;
    owner.await.expect("tool owner");
    assert!(result.contains("succeeded"));
}

#[test]
fn images_already_in_the_turn_count_against_what_one_request_may_carry() {
    let png = |bytes: usize| {
        rmcp::model::CallToolResult::success(vec![rmcp::model::ContentBlock::image(
            "A".repeat(bytes / 3 * 4),
            "image/png",
        )])
    };
    let mb = 1024 * 1024;
    let (_, images) = tool_result_text(&png(4 * mb), &DEEPSEEK_SPEC, Some(12 * mb)).unwrap();
    assert_eq!(images.len(), 1);
    let sent = json!({"role": "user", "content": [
        {"type": "image_url", "image_url": {"url": "A".repeat(10 * mb / 3 * 4)}}
    ]});
    let remaining = image_budget(true, &[sent], &[]).unwrap();
    assert!(remaining < 3 * mb, "{remaining}");
    let (refused, images) =
        tool_result_text(&png(4 * mb), &DEEPSEEK_SPEC, Some(remaining)).unwrap();
    assert!(images.is_empty());
    let value: Value = serde_json::from_str(&refused).unwrap();
    assert_eq!(value["error"]["code"], "room_tool_image_too_large");
    assert_eq!(image_budget(false, &[], &[]), None);
}
