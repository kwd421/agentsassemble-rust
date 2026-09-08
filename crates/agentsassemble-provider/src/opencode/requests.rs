//! Native `OpenCode` approval/question replies; the room retains ownership of human authority.
use agentsassemble_domain::{
    ProviderRequest, ProviderRequestKind, ProviderRequestOption, ProviderRequestPrompt,
    ProviderRequestQuestion, ProviderRequestResolution, redact_persisted_diagnostic_text,
};
use serde_json::{Value, json};
use uuid::Uuid;

use super::{OpenCodeDriver, REQUEST_TIMEOUT};
use crate::{
    opencode_protocol::{clean_session_id, protocol_error, provider_request_error},
    runtime::{DriverError, ProviderTurnRequest},
};

pub(super) async fn handle(
    driver: &mut OpenCodeDriver,
    session_id: &str,
    turn: &ProviderTurnRequest,
    event: &Value,
) -> Result<(), DriverError> {
    let mapped = NativeRequest::parse(event)?;
    let ingress = turn.request_ingress.as_ref().ok_or_else(unavailable)?;
    let mut exchange = ingress
        .open(
            session_id,
            turn.turn_generation,
            &turn.execution_id,
            mapped.request.clone(),
        )
        .await
        .map_err(|_| unavailable())?;
    let resolution = exchange.receive().await.map_err(|_| unavailable())?;
    let payload = mapped.response(&resolution)?;
    let connection = driver.connect_owned_peer().await?;
    let response = connection
        .post_json(&mapped.path, &payload, REQUEST_TIMEOUT)
        .await;
    let delivered = response
        .as_ref()
        .is_ok_and(|response| response.status.is_success());
    exchange
        .complete(delivered)
        .await
        .map_err(|_| unavailable())?;
    if delivered {
        Ok(())
    } else {
        Err(provider_request_error())
    }
}

struct NativeRequest {
    request: ProviderRequest,
    path: String,
}

impl NativeRequest {
    fn parse(event: &Value) -> Result<Self, DriverError> {
        let properties = event.get("properties").ok_or_else(protocol_error)?;
        let id = request_id(properties)?;
        let (kind, title, description, prompt, category) =
            match event.get("type").and_then(Value::as_str) {
                Some("permission.asked") => {
                    let permission = text(properties, "permission")?;
                    let patterns = strings(properties, "patterns")?;
                    let always = strings(properties, "always")?;
                    let mut options = vec![option("once", "Allow once", "allow_once", "")];
                    if !always.is_empty() {
                        options.push(option(
                            "always",
                            "Always allow in this project",
                            "allow_always",
                            &display(&always.join(", ")),
                        ));
                    }
                    options.push(option("reject", "Reject", "reject_once", ""));
                    (
                        ProviderRequestKind::Permission,
                        format!("OpenCode requests {permission}"),
                        display(&patterns.join(", ")),
                        ProviderRequestPrompt::Option { options },
                        "permission",
                    )
                }
                Some("question.asked") => (
                    ProviderRequestKind::UserInput,
                    "OpenCode needs your answer".to_owned(),
                    String::new(),
                    ProviderRequestPrompt::Answers {
                        questions: questions(properties)?,
                    },
                    "question",
                ),
                _ => return Err(protocol_error()),
            };
        let request = ProviderRequest {
            provider_request_id: Uuid::new_v4(),
            request_kind: kind,
            title: display(&title),
            description,
            timeout_seconds: 600,
            prompt,
        };
        if !request.is_valid() {
            return Err(protocol_error());
        }
        Ok(Self {
            request,
            path: format!("/{category}/{id}/reply"),
        })
    }

    fn response(&self, resolution: &ProviderRequestResolution) -> Result<Value, DriverError> {
        if self.request.durable_resolution(resolution).is_none() {
            return Err(protocol_error());
        }
        match (&self.request.prompt, resolution) {
            (
                ProviderRequestPrompt::Option { .. },
                ProviderRequestResolution::Option { option_id },
            ) => Ok(json!({"reply": option_id})),
            (
                ProviderRequestPrompt::Answers { questions },
                ProviderRequestResolution::Answers { answers },
            ) => {
                let ordered = questions
                    .iter()
                    .map(|question| answers.get(&question.id).ok_or_else(protocol_error))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(json!({"answers": ordered}))
            }
            _ => Err(protocol_error()),
        }
    }
}

fn questions(properties: &Value) -> Result<Vec<ProviderRequestQuestion>, DriverError> {
    properties
        .get("questions")
        .and_then(Value::as_array)
        .ok_or_else(protocol_error)?
        .iter()
        .enumerate()
        .map(|(index, question)| {
            let options = match question.get("options").filter(|value| !value.is_null()) {
                Some(value) => value
                    .as_array()
                    .ok_or_else(protocol_error)?
                    .iter()
                    .enumerate()
                    .map(|(index, value)| {
                        Ok(option(
                            &index.to_string(),
                            text(value, "label")?,
                            "answer",
                            optional_text(value, "description")?,
                        ))
                    })
                    .collect::<Result<_, DriverError>>()?,
                None => Vec::new(),
            };
            Ok(ProviderRequestQuestion {
                id: format!("question-{index}"),
                header: optional_text(question, "header")?.to_owned(),
                question: text(question, "question")?.to_owned(),
                options,
                multiple: boolean(question, "multiple", false)?,
                is_other: boolean(question, "custom", true)?,
                is_secret: false,
            })
        })
        .collect()
}

fn request_id(properties: &Value) -> Result<String, DriverError> {
    let id = properties
        .get("id")
        .or_else(|| properties.get("requestID"))
        .and_then(Value::as_str)
        .and_then(clean_session_id)
        .ok_or_else(protocol_error)?;
    if properties
        .get("requestID")
        .is_some_and(|other| other.as_str() != Some(&id))
    {
        return Err(protocol_error());
    }
    Ok(id)
}
fn option(id: &str, label: &str, kind: &str, description: &str) -> ProviderRequestOption {
    ProviderRequestOption {
        id: id.to_owned(),
        label: label.to_owned(),
        kind: kind.to_owned(),
        description: description.to_owned(),
    }
}
fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str, DriverError> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(protocol_error)
}
fn optional_text<'a>(value: &'a Value, key: &str) -> Result<&'a str, DriverError> {
    match value.get(key).filter(|value| !value.is_null()) {
        Some(value) => value.as_str().ok_or_else(protocol_error),
        None => Ok(""),
    }
}
fn strings<'a>(value: &'a Value, key: &str) -> Result<Vec<&'a str>, DriverError> {
    match value.get(key).filter(|value| !value.is_null()) {
        Some(value) => value
            .as_array()
            .ok_or_else(protocol_error)?
            .iter()
            .map(|value| value.as_str().ok_or_else(protocol_error))
            .collect(),
        None => Ok(Vec::new()),
    }
}
fn boolean(value: &Value, key: &str, default: bool) -> Result<bool, DriverError> {
    match value.get(key) {
        Some(value) => value.as_bool().ok_or_else(protocol_error),
        None => Ok(default),
    }
}
fn display(value: &str) -> String {
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
const fn unavailable() -> DriverError {
    DriverError::new(
        "provider_request_unavailable",
        "The OpenCode interactive request could not complete its live owner response.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opencode_questions_preserve_order_and_offered_answer_rules() -> Result<(), DriverError> {
        let mapped = NativeRequest::parse(&json!({"type": "question.asked", "properties": {
            "id": "q-1", "questions": [
                {"question": "Pick two", "multiple": true, "custom": false, "options": [{"label":"A"}, {"label":"B"}]},
                {"question": "Enter text", "custom": true}
            ]
        }}))?;
        let answers = [
            (
                "question-0".to_owned(),
                vec!["B".to_owned(), "A".to_owned()],
            ),
            ("question-1".to_owned(), vec!["text".to_owned()]),
        ]
        .into();
        assert_eq!(mapped.path, "/question/q-1/reply");
        assert_eq!(
            mapped.response(&ProviderRequestResolution::Answers { answers })?,
            json!({"answers": [["B", "A"], ["text"]]})
        );
        assert!(
            mapped
                .response(&ProviderRequestResolution::Answers {
                    answers: std::collections::BTreeMap::default()
                })
                .is_err()
        );
        let permission = NativeRequest::parse(
            &json!({"type": "permission.asked", "properties": {"id": "p-1", "permission":"bash", "patterns":[], "always":[]}}),
        )?;
        assert!(
            permission
                .response(&ProviderRequestResolution::Option {
                    option_id: "always".to_owned()
                })
                .is_err()
        );
        assert!(
            NativeRequest::parse(
                &json!({"type":"permission.asked", "properties":{"id":"p-1", "requestID":"p-2"}})
            )
            .is_err()
        );
        Ok(())
    }
}
