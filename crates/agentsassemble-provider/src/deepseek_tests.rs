use agentsassemble_domain::RoomRandomResult;
use serde_json::json;

use super::{RemoteOpenAiDriver, assistant_value, validate_completion};
use crate::room_portal::RoomObservationStart;
use crate::{
    credentials::ProviderCredentialStore,
    deepseek::DEEPSEEK_SPEC,
    openai_stream::{AssistantMessage, OpenAiStreamCompletion, ToolCall, ToolFunction},
    room_portal::{ProviderRoomToolIngress, ProviderRoomToolResult},
};

fn tool_call(id: &str, name: &str, arguments: &serde_json::Value) -> ToolCall {
    ToolCall {
        id: id.to_owned(),
        kind: "function",
        function: ToolFunction {
            name: name.to_owned(),
            arguments: arguments.to_string(),
        },
    }
}

#[test]
fn thinking_tool_transaction_preserves_exact_authority() {
    let response = OpenAiStreamCompletion {
        id: "chatcmpl-1".to_owned(),
        model: "deepseek-v4-flash".to_owned(),
        finish_reason: "tool_calls".to_owned(),
        message: AssistantMessage {
            role: "assistant",
            content: None,
            reasoning_content: Some("private reasoning".to_owned()),
            tool_calls: vec![tool_call("call-1", "read_discussion", &json!({}))],
        },
        usage: None,
    };
    assert!(validate_completion(&response, "deepseek-v4-flash", &DEEPSEEK_SPEC).is_ok());
    let message = &response.message;
    let replay = assistant_value(message, DEEPSEEK_SPEC.retain_reasoning);
    assert_eq!(replay["role"], "assistant");
    assert_eq!(replay["content"], "");
    assert_eq!(replay["reasoning_content"], "private reasoning");
}

#[tokio::test]
async fn committed_random_tool_keeps_the_turn_replay_unsafe() {
    let mut driver =
        RemoteOpenAiDriver::launch(&DEEPSEEK_SPEC, ProviderCredentialStore::production())
            .await
            .unwrap_or_else(|error| panic!("launch in-process portal: {}", error.error));
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
            .await
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

#[tokio::test]
async fn a_tool_the_room_does_not_offer_is_answered_instead_of_ending_the_session() {
    let mut driver =
        RemoteOpenAiDriver::launch(&DEEPSEEK_SPEC, ProviderCredentialStore::production())
            .await
            .unwrap_or_else(|error| panic!("launch in-process portal: {}", error.error));
    let (ingress, _commands) = ProviderRoomToolIngress::channel(1);
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
            room_view: "#1 Human: chat only",
            attachment_ids: &[],
            attachment_ingress: None,
            allowed_agent_ids: &[],
            tabletop_tools: false,
            tool_ingress: Some(ingress),
        })
        .unwrap_or_else(|error| panic!("begin observation: {error}"));

    // Dice exist only in tabletop rooms, so this call never reaches the room.
    let unavailable = driver
        .execute_tool(
            tool_call(
                "dice",
                "roll_dice",
                &json!({"notation": "1d6", "reason": ""}),
            ),
            false,
        )
        .await
        .unwrap_or_else(|error| panic!("answer an unavailable tool: {error}"));
    assert!(!unavailable.terminal);
    assert!(unavailable.result.contains("room_tool_unavailable"));

    let malformed = driver
        .execute_tool(
            ToolCall {
                id: "broken".to_owned(),
                kind: "function",
                function: ToolFunction {
                    name: "read_discussion".to_owned(),
                    arguments: "not json".to_owned(),
                },
            },
            false,
        )
        .await
        .unwrap_or_else(|error| panic!("answer malformed arguments: {error}"));
    assert!(!malformed.terminal);
    assert!(malformed.result.contains("room_tool_arguments_invalid"));
    assert!(!driver.turn_effect_uncertain);
}

#[test]
fn incomplete_or_inconsistent_completion_cannot_enter_room_tools() {
    let fixture = |finish_reason: &str, model: &str| OpenAiStreamCompletion {
        id: "chatcmpl-1".to_owned(),
        model: model.to_owned(),
        finish_reason: finish_reason.to_owned(),
        message: AssistantMessage {
            role: "assistant",
            content: None,
            reasoning_content: Some("bounded reasoning".to_owned()),
            tool_calls: vec![tool_call("call-1", "read_discussion", &json!({}))],
        },
        usage: None,
    };

    assert!(
        validate_completion(
            &fixture("tool_calls", "deepseek-v4-flash"),
            "deepseek-v4-flash",
            &DEEPSEEK_SPEC,
        )
        .is_ok()
    );
    for response in [
        fixture("length", "deepseek-v4-flash"),
        fixture("content_filter", "deepseek-v4-flash"),
        fixture("insufficient_system_resource", "deepseek-v4-flash"),
        fixture("stop", "deepseek-v4-flash"),
        fixture("tool_calls", "substituted-model"),
    ] {
        assert!(validate_completion(&response, "deepseek-v4-flash", &DEEPSEEK_SPEC).is_err());
    }
}

#[test]
fn a_reply_cut_off_at_the_response_limit_is_reported_as_its_own_outcome() {
    let plain = |finish_reason: &str| OpenAiStreamCompletion {
        id: "chatcmpl-2".to_owned(),
        model: "deepseek-v4-flash".to_owned(),
        finish_reason: finish_reason.to_owned(),
        message: AssistantMessage {
            role: "assistant",
            content: Some("bounded answer".to_owned()),
            reasoning_content: None,
            tool_calls: Vec::new(),
        },
        usage: None,
    };

    let Err(truncated) = validate_completion(&plain("length"), "deepseek-v4-flash", &DEEPSEEK_SPEC)
    else {
        panic!("a reply stopped at the limit must not pass validation");
    };
    assert_eq!(truncated.code, "provider_output_truncated");
    // Every other inconsistency stays a protocol fault, and a finished reply still passes.
    let Err(malformed) = validate_completion(
        &plain("content_filter"),
        "deepseek-v4-flash",
        &DEEPSEEK_SPEC,
    ) else {
        panic!("an unexpected finish reason must not pass validation");
    };
    assert_eq!(malformed.code, "provider_protocol_invalid");
    assert!(validate_completion(&plain("stop"), "deepseek-v4-flash", &DEEPSEEK_SPEC).is_ok());
}
