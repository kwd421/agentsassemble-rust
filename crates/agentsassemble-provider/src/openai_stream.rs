use std::collections::{BTreeMap, HashSet};

use bytes::Bytes;
use eventsource_stream::{EventStreamError, Eventsource};
use futures_util::{Stream, StreamExt};
use reqwest::{RequestBuilder, StatusCode, header};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

pub(crate) const MAX_REQUEST_BYTES: usize = 256_000 - 16_384 - 32_768;
const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;
const MAX_TOOL_CALLS: usize = 16;
const MAX_TOOL_ARGUMENT_BYTES: usize = 64 * 1024;
const MAX_IDENTIFIER_BYTES: usize = 128;
const MAX_FINISH_REASON_BYTES: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub(crate) enum OpenAiStreamError {
    #[error("the provider request context is too large")]
    ContextLimit,
    #[error("the provider rejected its credential")]
    CredentialRejected,
    #[error("the provider rate-limited the request")]
    RateLimited,
    #[error("the provider returned an unsuccessful HTTP status")]
    Http,
    #[error("the provider request did not complete")]
    Transport,
    #[error("the provider returned an oversized response")]
    ResponseTooLarge,
    #[error("the provider returned an invalid streaming response")]
    InvalidResponse,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OpenAiStreamCompletion {
    pub(crate) id: String,
    pub(crate) model: String,
    pub(crate) finish_reason: String,
    pub(crate) message: AssistantMessage,
    pub(crate) usage: Option<OpenAiUsage>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct AssistantMessage {
    pub(crate) role: &'static str,
    pub(crate) content: Option<String>,
    pub(crate) reasoning_content: Option<String>,
    pub(crate) tool_calls: Vec<ToolCall>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ToolCall {
    pub(crate) id: String,
    #[serde(rename = "type")]
    pub(crate) kind: &'static str,
    pub(crate) function: ToolFunction,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ToolFunction {
    pub(crate) name: String,
    pub(crate) arguments: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OpenAiUsage {
    pub(crate) prompt: u64,
    pub(crate) completion: u64,
    pub(crate) total: u64,
}

#[derive(Debug, Deserialize)]
struct StreamChunk {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    choices: Vec<StreamChoice>,
    #[serde(default)]
    usage: Option<UsageChunk>,
}

#[derive(Debug, Deserialize)]
struct StreamChoice {
    index: u32,
    #[serde(default)]
    delta: Value,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UsageChunk {
    #[serde(rename = "prompt_tokens")]
    prompt: u64,
    #[serde(rename = "completion_tokens")]
    completion: u64,
    #[serde(rename = "total_tokens")]
    total: u64,
}

#[derive(Debug, Deserialize)]
struct ToolCallDelta {
    index: usize,
    #[serde(default)]
    id: Option<String>,
    #[serde(rename = "type", default)]
    kind: Option<String>,
    #[serde(default)]
    function: Option<ToolFunctionDelta>,
}

#[derive(Debug, Deserialize)]
struct ToolFunctionDelta {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

#[derive(Default)]
struct CompletionAccumulator {
    id: String,
    model: String,
    finish_reason: String,
    content: String,
    reasoning: String,
    tools: BTreeMap<usize, ToolCallAccumulator>,
    usage: Option<OpenAiUsage>,
}

#[derive(Default)]
struct ToolCallAccumulator {
    id: String,
    name: String,
    arguments: String,
}

pub(crate) async fn send_chat_completion(
    request: RequestBuilder,
    body: Vec<u8>,
) -> Result<OpenAiStreamCompletion, OpenAiStreamError> {
    if body.len() > MAX_REQUEST_BYTES {
        return Err(OpenAiStreamError::ContextLimit);
    }
    let response = request
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ACCEPT, "text/event-stream")
        .body(body)
        .send()
        .await
        .map_err(|_| OpenAiStreamError::Transport)?;
    match response.status() {
        status if status.is_success() => {}
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
            return Err(OpenAiStreamError::CredentialRejected);
        }
        StatusCode::TOO_MANY_REQUESTS => return Err(OpenAiStreamError::RateLimited),
        _ => return Err(OpenAiStreamError::Http),
    }
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    if !content_type
        .split(';')
        .next()
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("text/event-stream"))
    {
        return Err(OpenAiStreamError::InvalidResponse);
    }
    let mut observed = 0_usize;
    let stream = response.bytes_stream().map(move |item| {
        let bytes = item.map_err(|_| OpenAiStreamError::Transport)?;
        observed = observed.saturating_add(bytes.len());
        if observed > MAX_RESPONSE_BYTES {
            return Err(OpenAiStreamError::ResponseTooLarge);
        }
        Ok(bytes)
    });
    decode_events(stream).await
}

async fn decode_events<S>(stream: S) -> Result<OpenAiStreamCompletion, OpenAiStreamError>
where
    S: Stream<Item = Result<Bytes, OpenAiStreamError>>,
{
    let events = stream.eventsource();
    futures_util::pin_mut!(events);
    let mut accumulator = CompletionAccumulator::default();
    let mut done = false;
    while let Some(event) = events.next().await {
        let event = event.map_err(|error| match error {
            EventStreamError::Transport(error) => error,
            EventStreamError::Utf8(_) | EventStreamError::Parser(_) => {
                OpenAiStreamError::InvalidResponse
            }
        })?;
        if event.event != "message" {
            return Err(OpenAiStreamError::InvalidResponse);
        }
        if event.data == "[DONE]" {
            done = true;
            break;
        }
        let chunk = serde_json::from_str::<StreamChunk>(&event.data)
            .map_err(|_| OpenAiStreamError::InvalidResponse)?;
        accumulator.push(chunk)?;
    }
    if !done {
        return Err(OpenAiStreamError::InvalidResponse);
    }
    accumulator.finish()
}

impl CompletionAccumulator {
    fn push(&mut self, chunk: StreamChunk) -> Result<(), OpenAiStreamError> {
        merge_identifier(&mut self.id, chunk.id)?;
        merge_identifier(&mut self.model, chunk.model)?;
        if let Some(usage) = chunk.usage {
            let usage = OpenAiUsage {
                prompt: usage.prompt,
                completion: usage.completion,
                total: usage.total,
            };
            if self
                .usage
                .replace(usage)
                .is_some_and(|prior| prior != usage)
            {
                return Err(OpenAiStreamError::InvalidResponse);
            }
        }
        if chunk.choices.is_empty() {
            return Ok(());
        }
        let [choice] = chunk.choices.as_slice() else {
            return Err(OpenAiStreamError::InvalidResponse);
        };
        if choice.index != 0 {
            return Err(OpenAiStreamError::InvalidResponse);
        }
        if let Some(reason) = choice.finish_reason.as_deref() {
            if !valid_text(reason, MAX_FINISH_REASON_BYTES)
                || (!self.finish_reason.is_empty() && self.finish_reason != reason)
            {
                return Err(OpenAiStreamError::InvalidResponse);
            }
            reason.clone_into(&mut self.finish_reason);
        }
        let delta = choice
            .delta
            .as_object()
            .ok_or(OpenAiStreamError::InvalidResponse)?;
        if delta
            .get("role")
            .is_some_and(|role| role.as_str() != Some("assistant"))
        {
            return Err(OpenAiStreamError::InvalidResponse);
        }
        append_optional_text(delta.get("content"), &mut self.content)?;
        let mut reasoning_value = None;
        for key in ["reasoning_content", "reasoning", "thinking"] {
            if let Some(value) = delta.get(key).filter(|value| !value.is_null())
                && reasoning_value.replace(value).is_some()
            {
                return Err(OpenAiStreamError::InvalidResponse);
            }
        }
        append_optional_text(reasoning_value, &mut self.reasoning)?;
        if let Some(tool_calls) = delta.get("tool_calls") {
            let calls = serde_json::from_value::<Vec<ToolCallDelta>>(tool_calls.clone())
                .map_err(|_| OpenAiStreamError::InvalidResponse)?;
            for call in calls {
                self.push_tool_delta(call)?;
            }
        }
        Ok(())
    }

    fn push_tool_delta(&mut self, delta: ToolCallDelta) -> Result<(), OpenAiStreamError> {
        if delta.index >= MAX_TOOL_CALLS {
            return Err(OpenAiStreamError::InvalidResponse);
        }
        let call = self.tools.entry(delta.index).or_default();
        if let Some(kind) = delta.kind.as_deref().filter(|kind| !kind.is_empty())
            && kind != "function"
        {
            return Err(OpenAiStreamError::InvalidResponse);
        }
        if let Some(id) = delta.id {
            merge_identifier(&mut call.id, Some(id))?;
        }
        if let Some(function) = delta.function {
            if let Some(name) = function.name {
                call.name.push_str(&name);
                if !valid_text(&call.name, MAX_IDENTIFIER_BYTES) {
                    return Err(OpenAiStreamError::InvalidResponse);
                }
            }
            if let Some(arguments) = function.arguments {
                call.arguments.push_str(&arguments);
                if call.arguments.len() > MAX_TOOL_ARGUMENT_BYTES {
                    return Err(OpenAiStreamError::InvalidResponse);
                }
            }
        }
        Ok(())
    }

    fn finish(self) -> Result<OpenAiStreamCompletion, OpenAiStreamError> {
        if !valid_text(&self.id, MAX_IDENTIFIER_BYTES)
            || !valid_text(&self.model, MAX_IDENTIFIER_BYTES)
            || !valid_text(&self.finish_reason, MAX_FINISH_REASON_BYTES)
        {
            return Err(OpenAiStreamError::InvalidResponse);
        }
        let mut tool_calls = Vec::with_capacity(self.tools.len());
        let mut tool_ids = HashSet::with_capacity(self.tools.len());
        for (expected, (index, call)) in self.tools.into_iter().enumerate() {
            if index != expected
                || !valid_text(&call.id, MAX_IDENTIFIER_BYTES)
                || !tool_ids.insert(call.id.clone())
                || !valid_text(&call.name, MAX_IDENTIFIER_BYTES)
                || call.arguments.len() > MAX_TOOL_ARGUMENT_BYTES
            {
                return Err(OpenAiStreamError::InvalidResponse);
            }
            tool_calls.push(ToolCall {
                id: call.id,
                kind: "function",
                function: ToolFunction {
                    name: call.name,
                    arguments: call.arguments,
                },
            });
        }
        Ok(OpenAiStreamCompletion {
            id: self.id,
            model: self.model,
            finish_reason: self.finish_reason,
            message: AssistantMessage {
                role: "assistant",
                content: (!self.content.is_empty()).then_some(self.content),
                reasoning_content: (!self.reasoning.is_empty()).then_some(self.reasoning),
                tool_calls,
            },
            usage: self.usage,
        })
    }
}

fn merge_identifier(target: &mut String, value: Option<String>) -> Result<(), OpenAiStreamError> {
    let Some(value) = value.filter(|value| !value.is_empty()) else {
        return Ok(());
    };
    if !valid_text(&value, MAX_IDENTIFIER_BYTES) || (!target.is_empty() && target != &value) {
        return Err(OpenAiStreamError::InvalidResponse);
    }
    if target.is_empty() {
        *target = value;
    }
    Ok(())
}

fn append_optional_text(
    value: Option<&Value>,
    target: &mut String,
) -> Result<(), OpenAiStreamError> {
    let Some(value) = value else {
        return Ok(());
    };
    if value.is_null() {
        return Ok(());
    }
    let value = value.as_str().ok_or(OpenAiStreamError::InvalidResponse)?;
    target.push_str(value);
    Ok(())
}

fn valid_text(value: &str, max_bytes: usize) -> bool {
    !value.is_empty()
        && value.len() <= max_bytes
        && value.trim() == value
        && !value.chars().any(char::is_control)
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use futures_util::stream;
    use serde_json::json;

    use super::{OpenAiStreamError, decode_events};

    fn event(value: &serde_json::Value) -> String {
        format!("data: {value}\n\n")
    }

    #[tokio::test]
    async fn fragmented_sse_yields_one_exact_completion() {
        let payload = [
            event(&json!({
                "id": "chat-1",
                "model": "model-1",
                "choices": [{
                    "index": 0,
                    "delta": {
                        "role": "assistant",
                        "reasoning_content": "private ",
                        "tool_calls": [{
                            "index": 0,
                            "id": "call-1",
                            "type": "function",
                            "function": {"name": "read_", "arguments": "{"}
                        }]
                    },
                    "finish_reason": null
                }]
            })),
            event(&json!({
                "id": "chat-1",
                "model": "model-1",
                "choices": [{
                    "index": 0,
                    "delta": {
                        "reasoning_content": "reasoning",
                        "tool_calls": [{
                            "index": 0,
                            "function": {"name": "discussion", "arguments": "}"}
                        }]
                    },
                    "finish_reason": "tool_calls"
                }]
            })),
            event(&json!({
                "id": "chat-1",
                "model": "model-1",
                "choices": [],
                "usage": {"prompt_tokens": 3, "completion_tokens": 2, "total_tokens": 5}
            })),
            "data: [DONE]\n\n".to_owned(),
        ]
        .concat();
        let midpoint = payload.len() / 2;
        let stream = stream::iter([
            Ok(Bytes::copy_from_slice(&payload.as_bytes()[..midpoint])),
            Ok(Bytes::copy_from_slice(&payload.as_bytes()[midpoint..])),
        ]);
        let completion = decode_events(stream)
            .await
            .unwrap_or_else(|error| panic!("decode stream: {error}"));

        assert_eq!(completion.id, "chat-1");
        assert_eq!(completion.model, "model-1");
        assert_eq!(completion.finish_reason, "tool_calls");
        assert_eq!(
            completion.message.reasoning_content.as_deref(),
            Some("private reasoning")
        );
        assert_eq!(
            completion.message.tool_calls[0].function.name,
            "read_discussion"
        );
        assert_eq!(completion.message.tool_calls[0].function.arguments, "{}");
        assert_eq!(completion.usage.map(|usage| usage.total), Some(5));
    }

    #[tokio::test]
    async fn eof_without_done_never_claims_completion() {
        let payload = event(&json!({
            "id": "chat-1",
            "model": "model-1",
            "choices": [{
                "index": 0,
                "delta": {"content": "partial"},
                "finish_reason": "stop"
            }]
        }));
        let error = decode_events(stream::iter([Ok(Bytes::from(payload))]))
            .await
            .err()
            .unwrap_or_else(|| panic!("truncated stream must fail"));
        assert_eq!(error, OpenAiStreamError::InvalidResponse);
    }
}
