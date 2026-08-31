use agentsassemble_domain::RoomRandomResult;
use serde_json::json;

use super::{
    AssistantMessage, CompletionResponse, DeepSeekDriver, RoomObservationStart, ToolCall,
    ToolFunction, allowed_tool, assistant_value, validate_completion, validate_tool_calls,
};
use crate::{
    credentials::ProviderCredentialStore,
    room_portal::{ProviderRoomToolIngress, ProviderRoomToolResult},
};

fn tool_call(id: &str, name: &str, arguments: &serde_json::Value) -> ToolCall {
    ToolCall {
        id: id.to_owned(),
        kind: "function".to_owned(),
        function: ToolFunction {
            name: name.to_owned(),
            arguments: arguments.to_string(),
        },
    }
}

#[test]
fn thinking_tool_transaction_preserves_exact_authority() {
    let response: CompletionResponse = serde_json::from_value(json!({
        "id": "chatcmpl-1",
        "model": "deepseek-v4-flash",
        "choices": [{
            "index": 0,
            "finish_reason": "tool_calls",
            "message": {
            "role": "assistant",
            "content": null,
            "reasoning_content": "private reasoning",
            "tool_calls": [{
                "id": "call-1",
                "type": "function",
                "function": {"name": "read_discussion", "arguments": "{}"}
            }]
        }}]
    }))
    .unwrap_or_else(|error| panic!("decode fixture: {error}"));
    assert!(validate_completion(&response, "deepseek-v4-flash").is_ok());
    let message: &AssistantMessage = &response.choices[0].message;
    assert!(validate_tool_calls(&message.tool_calls).is_ok());
    let replay = assistant_value(message);
    assert_eq!(replay["role"], "assistant");
    assert_eq!(replay["content"], "");
    assert_eq!(replay["reasoning_content"], "private reasoning");
    assert!(allowed_tool("read_discussion", false));
    assert!(allowed_tool("search_messages", false));
    assert!(allowed_tool("read_message_context", false));
    assert!(allowed_tool("create_vote", false));
    assert!(allowed_tool("cast_vote", false));
    assert!(allowed_tool("withdraw_vote", false));
    assert!(allowed_tool("close_vote", false));
    assert!(!allowed_tool("read_attachment", true));
    assert!(!allowed_tool("roll_dice", false));
    assert!(allowed_tool("roll_dice", true));
}

#[tokio::test]
async fn committed_random_tool_keeps_the_turn_replay_unsafe() {
    let mut driver = DeepSeekDriver::launch(ProviderCredentialStore::production())
        .await
        .unwrap_or_else(|error| panic!("launch in-process portal: {error}"));
    let (ingress, mut commands) = ProviderRoomToolIngress::channel(1);
    driver
        .portal
        .as_ref()
        .unwrap_or_else(|| panic!("portal must be present"))
        .begin_observation(RoomObservationStart {
            session_id: "deepseek-test-session",
            turn_id: "deepseek-test-turn",
            input_up_to_seq: 1,
            durable_turn_generation: 1,
            execution_id: "00000000-0000-4000-8000-000000000099",
            room_view: "#1 Human: roll once",
            attachment_ids: &[],
            attachment_ingress: None,
            allowed_agent_ids: &[],
            tabletop_tools: true,
            tool_ingress: Some(ingress),
        })
        .unwrap_or_else(|error| panic!("begin observation: {error}"));
    driver
        .execute_tool(tool_call("read", "read_discussion", &json!({})), false)
        .await
        .unwrap_or_else(|error| panic!("read discussion: {error}"));
    let committed = tokio::spawn(async move {
        let mut command = commands
            .recv()
            .await
            .unwrap_or_else(|| panic!("receive room random command"));
        command
            .begin_execution()
            .unwrap_or_else(|error| panic!("begin random commit: {error}"));
        command.complete(Ok(ProviderRoomToolResult::Random(
            RoomRandomResult::RollDice {
                notation: "1d6".to_owned(),
                rolls: vec![4],
                modifier: 0,
                total: 4,
            },
        )));
    });
    driver
        .execute_tool(
            tool_call(
                "roll",
                "roll_dice",
                &json!({"notation": "1d6", "reason": ""}),
            ),
            true,
        )
        .await
        .unwrap_or_else(|error| panic!("execute committed random tool: {error}"));
    committed
        .await
        .unwrap_or_else(|error| panic!("join room random owner: {error}"));
    assert!(driver.turn_effect_uncertain);

    driver
        .execute_tool(
            tool_call(
                "rejected-after-commit",
                "roll_dice",
                &json!({"notation": "1d6", "reason": ""}),
            ),
            true,
        )
        .await
        .unwrap_or_else(|error| panic!("project explicit rejection: {error}"));
    assert!(driver.turn_effect_uncertain);

    driver.turn_effect_uncertain = false;
    driver
        .execute_tool(
            tool_call(
                "rejected-before-effect",
                "roll_dice",
                &json!({"notation": "1d6", "reason": ""}),
            ),
            true,
        )
        .await
        .unwrap_or_else(|error| panic!("project definitive rejection: {error}"));
    assert!(!driver.turn_effect_uncertain);
}

#[test]
fn incomplete_or_inconsistent_completion_cannot_enter_room_tools() {
    let fixture = |finish_reason: &str, role: &str, index: u32| {
        serde_json::from_value::<CompletionResponse>(json!({
            "id": "chatcmpl-1",
            "model": "deepseek-v4-flash",
            "choices": [{
                "index": index,
                "finish_reason": finish_reason,
                "message": {
                    "role": role,
                    "content": null,
                    "reasoning_content": "bounded reasoning",
                    "tool_calls": [{
                        "id": "call-1",
                        "type": "function",
                        "function": {"name": "read_discussion", "arguments": "{}"}
                    }]
                }
            }]
        }))
        .unwrap_or_else(|error| panic!("decode completion fixture: {error}"))
    };

    assert!(
        validate_completion(&fixture("tool_calls", "assistant", 0), "deepseek-v4-flash").is_ok()
    );
    for response in [
        fixture("length", "assistant", 0),
        fixture("content_filter", "assistant", 0),
        fixture("insufficient_system_resource", "assistant", 0),
        fixture("stop", "assistant", 0),
        fixture("tool_calls", "user", 0),
        fixture("tool_calls", "assistant", 1),
    ] {
        assert!(validate_completion(&response, "deepseek-v4-flash").is_err());
    }
}
