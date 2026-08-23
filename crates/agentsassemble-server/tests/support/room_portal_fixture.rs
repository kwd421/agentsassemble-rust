use std::{path::Path, time::Duration};

use rmcp::{
    ServiceExt,
    model::CallToolRequestParams,
    service::RunningService,
    transport::{
        StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use serde_json::{Map, json};

pub(super) fn script(
    transcript: &Path,
    portal_endpoint: &Path,
    portal_token: &Path,
    turn_seen: &Path,
    release_first: &Path,
    release_second: &Path,
) -> String {
    format!(
        r#"#!/bin/sh
umask 077
portal_url=
room_token_filtered=
shell_snapshot_disabled=
room_token_env_name=
for argument in "$@"
do
    case "$argument" in
        mcp_servers.agentsassemble_room.url=*)
            portal_url=${{argument#*=}}
            portal_url=${{portal_url#\"}}
            portal_url=${{portal_url%\"}}
            ;;
        mcp_servers.agentsassemble_room.bearer_token_env_var=*)
            room_token_env_name=${{argument#*=}}
            room_token_env_name=${{room_token_env_name#\"}}
            room_token_env_name=${{room_token_env_name%\"}}
            ;;
        shell_environment_policy.ignore_default_excludes=false)
            room_token_filtered=1
            ;;
        features.shell_snapshot=false)
            shell_snapshot_disabled=1
            ;;
    esac
done
test -n "$portal_url" || exit 40
case "$room_token_env_name" in
    AGENTSASSEMBLE_INTERNAL_ROOM_PORTAL_TOKEN_*) ;;
    *) exit 45 ;;
esac
case "$room_token_env_name" in
    *[!A-Z0-9_]*) exit 46 ;;
esac
eval "room_token=\${{${{room_token_env_name}}-}}"
test -n "$room_token" || exit 41
test "$room_token_filtered" = 1 || exit 43
test "$shell_snapshot_disabled" = 1 || exit 44
printf '%s' "$portal_url" > {endpoint}
printf '%s' "$room_token" > {token}
IFS= read -r initialize
printf '%s\n' "$initialize" >> {log}
printf '%s\n' '{{"jsonrpc":"2.0","id":1,"result":{{}}}}'
IFS= read -r initialized
printf '%s\n' "$initialized" >> {log}
IFS= read -r thread
printf '%s\n' "$thread" >> {log}
printf '%s\n' '{{"jsonrpc":"2.0","id":2,"result":{{"thread":{{"id":"thread-1"}}}}}}'
IFS= read -r turn_one
printf '%s\n' "$turn_one" >> {log}
printf '%s\n' '{{"jsonrpc":"2.0","id":"room-approval-1","method":"mcpServer/elicitation/request","params":{{"serverName":"agentsassemble_room","mode":"form","_meta":{{"codex_approval_kind":"mcp_tool_call"}}}}}}'
IFS= read -r approval_one
printf '%s\n' "$approval_one" >> {log}
case "$approval_one" in
    *'"action":"accept"'*'"content":{{}}'*) ;;
    *) exit 42 ;;
esac
printf '1' > {seen}
while [ ! -f {release_first} ]; do :; done
printf '%s\n' '{{"jsonrpc":"2.0","id":3,"result":{{"turn":{{"id":"provider-turn-1"}}}}}}'
printf '%s\n' '{{"jsonrpc":"2.0","method":"agent_message/completed","params":{{"threadId":"thread-1","turnId":"provider-turn-1","text":"ignored first assistant final"}}}}'
printf '%s\n' '{{"jsonrpc":"2.0","method":"turn/completed","params":{{"threadId":"thread-1","turnId":"provider-turn-1"}}}}'
IFS= read -r turn_two
printf '%s\n' "$turn_two" >> {log}
printf '2' > {seen}
while [ ! -f {release_second} ]; do :; done
printf '%s\n' '{{"jsonrpc":"2.0","id":4,"result":{{"turn":{{"id":"provider-turn-2"}}}}}}'
printf '%s\n' '{{"jsonrpc":"2.0","method":"agent_message/completed","params":{{"threadId":"thread-1","turnId":"provider-turn-2","text":"ignored second assistant final"}}}}'
printf '%s\n' '{{"jsonrpc":"2.0","method":"turn/completed","params":{{"threadId":"thread-1","turnId":"provider-turn-2"}}}}'
IFS= read -r forever
"#,
        log = quote(transcript),
        endpoint = quote(portal_endpoint),
        token = quote(portal_token),
        seen = quote(turn_seen),
        release_first = quote(release_first),
        release_second = quote(release_second),
    )
}

pub(super) async fn wait_for_value(path: &Path, description: &str) -> String {
    for _ in 0..500 {
        if let Ok(value) = std::fs::read_to_string(path)
            && !value.is_empty()
        {
            return value;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("RoomPortal {description} was not published");
}

pub(super) async fn wait_for_turn(path: &Path, expected: &str) {
    for _ in 0..500 {
        if std::fs::read_to_string(path).is_ok_and(|value| value == expected) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("provider fixture did not receive turn {expected}");
}

pub(super) async fn publish(endpoint: &str, token: &str, content: &str) -> String {
    let client = ()
        .serve(StreamableHttpClientTransport::from_config(
            StreamableHttpClientTransportConfig::with_uri(endpoint).auth_header(token),
        ))
        .await
        .unwrap_or_else(|error| panic!("connect room portal MCP: {error}"));
    let read = call_tool(&client, "read_discussion", json!({})).await;
    let view = read
        .content
        .first()
        .and_then(|content| content.as_text())
        .map_or_else(
            || panic!("RoomPortal read returned no text"),
            |content| content.text.clone(),
        );
    let result = call_tool(
        &client,
        "publish_message",
        json!({"content": content, "next_agent_id": ""}),
    )
    .await;
    assert_ne!(result.is_error, Some(true));
    let _ = client.cancel().await;
    view
}

async fn call_tool(
    client: &RunningService<rmcp::RoleClient, ()>,
    name: &'static str,
    arguments: serde_json::Value,
) -> rmcp::model::CallToolResult {
    let arguments = serde_json::from_value::<Map<String, serde_json::Value>>(arguments)
        .unwrap_or_else(|error| panic!("decode tool arguments: {error}"));
    client
        .call_tool(CallToolRequestParams::new(name).with_arguments(arguments))
        .await
        .unwrap_or_else(|error| panic!("call {name}: {error}"))
}

fn quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"))
}
