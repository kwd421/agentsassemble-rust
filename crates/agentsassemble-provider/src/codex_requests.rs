//! Codex request translation. Native policy objects remain live and never enter room history.
use std::collections::BTreeMap;

use agentsassemble_domain::{
    ProviderRequest, ProviderRequestKind, ProviderRequestOption, ProviderRequestPrompt,
    ProviderRequestQuestion, ProviderRequestResolution, redact_persisted_diagnostic_text,
};
use serde_json::{Value, json};
use uuid::Uuid;

use super::protocol_error;
use crate::runtime::{DriverError, ProviderTurnRequest};

pub(super) fn supported(message: &Value) -> bool {
    matches!(
        message.get("method").and_then(Value::as_str),
        Some(
            "item/commandExecution/requestApproval"
                | "item/fileChange/requestApproval"
                | "item/permissions/requestApproval"
                | "item/tool/requestUserInput"
                | "command_execution/request_approval"
                | "file_change/request_approval"
                | "permissions/request_approval"
        )
    )
}

pub(super) async fn handle(
    driver: &mut super::CodexDriver,
    session_id: &str,
    turn: &ProviderTurnRequest,
    message: &Value,
) -> Result<(), DriverError> {
    let id = message
        .get("id")
        .filter(|id| id.is_string() || id.is_i64() || id.is_u64())
        .ok_or_else(protocol_error)?;
    let mapped = MappedRequest::parse(message)?;
    let ingress = turn
        .request_ingress
        .as_ref()
        .ok_or_else(request_unavailable)?;
    let mut exchange = ingress
        .open(
            session_id,
            turn.turn_generation,
            &turn.execution_id,
            mapped.request.clone(),
        )
        .await
        .map_err(|_| request_unavailable())?;
    let resolution = exchange
        .receive()
        .await
        .map_err(|_| request_unavailable())?;
    let result = mapped.response(&resolution)?;
    let written = driver
        .write_message(&json!({"jsonrpc": "2.0", "id": id, "result": result}))
        .await;
    // JSON-RPC responses have no response ACK. Flushed native stdin is this delivery boundary;
    // the exchange separately waits for the room owner's durable delivery receipt.
    exchange
        .complete(written.is_ok())
        .await
        .map_err(|_| request_unavailable())?;
    written
}

struct MappedRequest {
    request: ProviderRequest,
    decisions: BTreeMap<String, Value>,
}

impl MappedRequest {
    fn parse(message: &Value) -> Result<Self, DriverError> {
        let method = message
            .get("method")
            .and_then(Value::as_str)
            .ok_or_else(protocol_error)?;
        let params = message
            .get("params")
            .filter(|params| params.is_object())
            .ok_or_else(protocol_error)?;
        let mut decisions = BTreeMap::new();
        let (title, kind, prompt) = match method {
            "item/tool/requestUserInput" => (
                "Codex needs your answer",
                ProviderRequestKind::UserInput,
                ProviderRequestPrompt::Answers {
                    questions: questions(params)?,
                },
            ),
            "item/permissions/requestApproval" | "permissions/request_approval" => {
                let options = permission_options(params, &mut decisions)?;
                (
                    "Codex requests additional permissions",
                    ProviderRequestKind::Permission,
                    ProviderRequestPrompt::Option { options },
                )
            }
            "item/commandExecution/requestApproval"
            | "command_execution/request_approval"
            | "item/fileChange/requestApproval"
            | "file_change/request_approval" => {
                let mut options = [
                    ("accept", "Allow once", "allow_once"),
                    (
                        "acceptForSession",
                        "Allow for this session",
                        "allow_session",
                    ),
                    ("decline", "Decline and continue", "decline"),
                    ("cancel", "Decline and stop turn", "cancel"),
                ]
                .into_iter()
                .map(|(id, label, kind)| {
                    decisions.insert(id.to_owned(), json!({"decision": id}));
                    option(id, label, kind, "")
                })
                .collect::<Vec<_>>();
                let command = matches!(
                    method,
                    "item/commandExecution/requestApproval" | "command_execution/request_approval"
                );
                if command {
                    amendments(params, &mut options, &mut decisions)?;
                }
                (
                    if command {
                        "Codex requests command execution"
                    } else {
                        "Codex requests file changes"
                    },
                    ProviderRequestKind::Permission,
                    ProviderRequestPrompt::Option { options },
                )
            }
            _ => return Err(protocol_error()),
        };
        let request = ProviderRequest {
            provider_request_id: Uuid::new_v4(),
            request_kind: kind,
            title: title.to_owned(),
            description: description(params)?,
            timeout_seconds: timeout(params)?,
            prompt,
        };
        if !request.is_valid() {
            return Err(protocol_error());
        }
        Ok(Self { request, decisions })
    }

    fn response(&self, resolution: &ProviderRequestResolution) -> Result<Value, DriverError> {
        if self.request.durable_resolution(resolution).is_none() {
            return Err(protocol_error());
        }
        match resolution {
            ProviderRequestResolution::Option { option_id } => self
                .decisions
                .get(option_id)
                .cloned()
                .ok_or_else(protocol_error),
            ProviderRequestResolution::Answers { answers } => Ok(json!({"answers": answers.iter()
                .map(|(id, values)| (id, json!({"answers": values}))).collect::<BTreeMap<_, _>>()})),
            ProviderRequestResolution::Acknowledge => Err(protocol_error()),
        }
    }
}

fn option(id: &str, label: &str, kind: &str, description: &str) -> ProviderRequestOption {
    ProviderRequestOption {
        id: id.to_owned(),
        label: label.to_owned(),
        kind: kind.to_owned(),
        description: description.to_owned(),
    }
}

fn amendments(
    params: &Value,
    options: &mut Vec<ProviderRequestOption>,
    decisions: &mut BTreeMap<String, Value>,
) -> Result<(), DriverError> {
    if let Some(amendment) = params
        .get("proposedExecpolicyAmendment")
        .filter(|value| !value.is_null())
    {
        let values = amendment
            .as_array()
            .filter(|values| !values.is_empty() && values.iter().all(Value::is_string))
            .ok_or_else(protocol_error)?;
        let id = "acceptWithExecpolicyAmendment";
        options.push(option(
            id,
            "Allow and add execution rule",
            "allow_policy",
            &display(&serde_json::to_string(values).map_err(|_| protocol_error())?),
        ));
        decisions.insert(id.to_owned(), json!({"decision": {"acceptWithExecpolicyAmendment": {"execpolicy_amendment": values}}}));
    }
    if let Some(amendments) = params
        .get("proposedNetworkPolicyAmendments")
        .filter(|value| !value.is_null())
    {
        let amendments = amendments
            .as_array()
            .filter(|values| values.len() <= 4)
            .ok_or_else(protocol_error)?;
        for (index, amendment) in amendments.iter().enumerate() {
            let host = amendment
                .get("host")
                .and_then(Value::as_str)
                .filter(|host| !host.is_empty())
                .ok_or_else(protocol_error)?;
            let action = amendment
                .get("action")
                .and_then(Value::as_str)
                .filter(|action| matches!(*action, "allow" | "deny"))
                .ok_or_else(protocol_error)?;
            let id = format!("network-policy-{index}");
            options.push(option(
                &id,
                &format!("Apply {action} network rule"),
                "allow_policy",
                &display(host),
            ));
            decisions.insert(id, json!({"decision": {"applyNetworkPolicyAmendment": {"network_policy_amendment": {"host": host, "action": action}}}}));
        }
    }
    Ok(())
}

fn questions(params: &Value) -> Result<Vec<ProviderRequestQuestion>, DriverError> {
    params
        .get("questions")
        .and_then(Value::as_array)
        .ok_or_else(protocol_error)?
        .iter()
        .map(|question| {
            let options = match question.get("options").filter(|value| !value.is_null()) {
                Some(value) => value
                    .as_array()
                    .ok_or_else(protocol_error)?
                    .iter()
                    .enumerate()
                    .map(|(index, value)| {
                        Ok(option(
                            &index.to_string(),
                            required(value, "label")?,
                            "answer",
                            optional(value, "description")?,
                        ))
                    })
                    .collect::<Result<_, DriverError>>()?,
                None => Vec::new(),
            };
            Ok(ProviderRequestQuestion {
                id: required(question, "id")?.to_owned(),
                header: optional(question, "header")?.to_owned(),
                question: required(question, "question")?.to_owned(),
                options,
                multiple: false,
                is_other: boolean(question, "isOther")?,
                is_secret: boolean(question, "isSecret")?,
            })
        })
        .collect()
}

fn required<'a>(value: &'a Value, key: &str) -> Result<&'a str, DriverError> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(protocol_error)
}
fn optional<'a>(value: &'a Value, key: &str) -> Result<&'a str, DriverError> {
    match value.get(key).filter(|value| !value.is_null()) {
        Some(value) => value.as_str().ok_or_else(protocol_error),
        None => Ok(""),
    }
}
fn boolean(value: &Value, key: &str) -> Result<bool, DriverError> {
    match value.get(key) {
        Some(value) => value.as_bool().ok_or_else(protocol_error),
        None => Ok(false),
    }
}
fn description(params: &Value) -> Result<String, DriverError> {
    let mut parts = vec![
        optional(params, "reason")?.to_owned(),
        optional(params, "command")?.to_owned(),
    ];
    if let Some(permissions) = params.get("permissions") {
        parts.push(serde_json::to_string(permissions).map_err(|_| protocol_error())?);
    }
    Ok(display(
        &parts
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join("; "),
    ))
}
fn display(value: &str) -> String {
    // Reuse the durable redaction owner, but do not truncate permission descriptions:
    // the domain rejects an oversized prompt instead of presenting partial authority.
    redact_persisted_diagnostic_text(value, value.chars().count())
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect()
}
fn timeout(params: &Value) -> Result<u16, DriverError> {
    match params
        .get("autoResolutionMs")
        .filter(|value| !value.is_null())
    {
        Some(value) => {
            let milliseconds = value.as_u64().ok_or_else(protocol_error)?;
            if milliseconds == 0 {
                return Ok(600);
            }
            u16::try_from((milliseconds / 1000).clamp(15, 900)).map_err(|_| protocol_error())
        }
        None => Ok(600),
    }
}

const fn request_unavailable() -> DriverError {
    DriverError::new(
        "provider_request_unavailable",
        "The Codex interactive request could not complete its live owner response.",
    )
}

fn permission_options(
    params: &Value,
    decisions: &mut BTreeMap<String, Value>,
) -> Result<Vec<ProviderRequestOption>, DriverError> {
    let permissions = params
        .get("permissions")
        .filter(|value| value.is_object())
        .ok_or_else(protocol_error)?;
    Ok([
        (
            "grant-turn",
            "Allow for this turn",
            "allow_once",
            permissions,
            "turn",
        ),
        (
            "grant-session",
            "Allow for this session",
            "allow_session",
            permissions,
            "session",
        ),
        ("deny", "Deny", "deny", &json!({}), "turn"),
    ]
    .into_iter()
    .map(|(id, label, kind, permissions, scope)| {
        decisions.insert(
            id.to_owned(),
            json!({"permissions": permissions, "scope": scope}),
        );
        option(id, label, kind, "")
    })
    .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_permission_choices_preserve_native_policy_and_reject_unknown_answers()
    -> Result<(), DriverError> {
        let mapped = MappedRequest::parse(
            &json!({"method": "item/commandExecution/requestApproval", "params": {
                "command": "echo --token fixture-private-value", "proposedExecpolicyAmendment": ["echo"],
                "proposedNetworkPolicyAmendments": [{"host": "example.test", "action": "allow"}]
            }}),
        )?;
        assert!(!mapped.request.description.contains("fixture-private-value"));
        for (id, decision) in [
            ("accept", json!("accept")),
            ("acceptForSession", json!("acceptForSession")),
            ("decline", json!("decline")),
            ("cancel", json!("cancel")),
            (
                "acceptWithExecpolicyAmendment",
                json!({"acceptWithExecpolicyAmendment": {"execpolicy_amendment": ["echo"]}}),
            ),
            (
                "network-policy-0",
                json!({"applyNetworkPolicyAmendment": {"network_policy_amendment": {"host": "example.test", "action": "allow"}}}),
            ),
        ] {
            assert_eq!(
                mapped.response(&ProviderRequestResolution::Option {
                    option_id: id.to_owned()
                })?,
                json!({"decision": decision})
            );
        }
        assert!(
            mapped
                .response(&ProviderRequestResolution::Option {
                    option_id: "unknown".to_owned()
                })
                .is_err()
        );
        let permissions = json!({"network": {"enabled": true}});
        let mapped = MappedRequest::parse(
            &json!({"method": "item/permissions/requestApproval", "params": {"permissions": permissions}}),
        )?;
        for (id, granted, scope) in [
            ("grant-turn", permissions.clone(), "turn"),
            ("grant-session", permissions, "session"),
            ("deny", json!({}), "turn"),
        ] {
            assert_eq!(
                mapped.response(&ProviderRequestResolution::Option {
                    option_id: id.to_owned()
                })?,
                json!({"permissions": granted, "scope": scope})
            );
        }
        assert!(MappedRequest::parse(&json!({"method": "item/tool/requestUserInput", "params": {"questions": [{"id":"q", "question":"Question", "isSecret":"false"}]}})).is_err());
        Ok(())
    }
}
