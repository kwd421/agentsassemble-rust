//! Native `OpenCode` approval/question replies; the room retains ownership of human authority.
use agentsassemble_domain::{
    ProviderRequest, ProviderRequestKind, ProviderRequestOption, ProviderRequestPrompt,
    ProviderRequestQuestion, ProviderRequestResolution, redact_persisted_diagnostic_text,
};
use serde_json::{Value, json};
use uuid::Uuid;

use super::{OpenCodeDriver, REQUEST_TIMEOUT};
use crate::{
    opencode_protocol::{OpenCodeApi, clean_session_id, protocol_error, provider_request_error},
    runtime::{DriverError, ProviderTurnRequest},
};

pub(super) async fn handle(
    driver: &mut OpenCodeDriver,
    session_id: &str,
    turn: &ProviderTurnRequest,
    event: &Value,
) -> Result<(), DriverError> {
    let mapped = NativeRequest::parse(event, driver.api)?;
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
    reply: ReplyShape,
}

enum ReplyShape {
    V1Permission,
    V2Permission,
    V1Question,
    V2Form(Vec<FormField>),
}

struct FormField {
    key: String,
    kind: FormFieldKind,
    choices: Vec<(String, String)>,
}

enum FormFieldKind {
    Text,
    Number,
    Integer,
    Boolean,
    Strings,
}

impl NativeRequest {
    fn parse(event: &Value, api: OpenCodeApi) -> Result<Self, DriverError> {
        let v2 = event.get("data").is_some();
        let properties = event
            .get("properties")
            .or_else(|| event.get("data"))
            .ok_or_else(protocol_error)?;
        let id = request_id(properties)?;
        let (kind, title, description, prompt, path, reply) =
            match event.get("type").and_then(Value::as_str) {
                Some("permission.asked") => {
                    let options = vec![
                        option("once", "Allow once", "allow_once", ""),
                        option("reject", "Reject", "reject_once", ""),
                    ];
                    if v2 && api == OpenCodeApi::V2 {
                        let action = text(properties, "action")?;
                        let resources = strings(properties, "resources")?;
                        let session_id = properties
                            .get("sessionID")
                            .and_then(Value::as_str)
                            .and_then(clean_session_id)
                            .ok_or_else(protocol_error)?;
                        (
                            ProviderRequestKind::Permission,
                            format!("OpenCode requests {action}"),
                            display(
                                &[action.to_owned(), resources.join(", ")]
                                    .into_iter()
                                    .filter(|part| !part.is_empty())
                                    .collect::<Vec<_>>()
                                    .join(" · "),
                            ),
                            ProviderRequestPrompt::Option { options },
                            format!("/api/session/{session_id}/permission/{id}/reply"),
                            ReplyShape::V2Permission,
                        )
                    } else {
                        let permission = text(properties, "permission")?;
                        let patterns = strings(properties, "patterns")?;
                        (
                            ProviderRequestKind::Permission,
                            format!("OpenCode requests {permission}"),
                            // The subject (permission name) belongs in the detail field so the
                            // owner-facing card can present it beside the localized title.
                            display(
                                &[permission.to_owned(), patterns.join(", ")]
                                    .into_iter()
                                    .filter(|part| !part.is_empty())
                                    .collect::<Vec<_>>()
                                    .join(" · "),
                            ),
                            ProviderRequestPrompt::Option { options },
                            format!("/permission/{id}/reply"),
                            ReplyShape::V1Permission,
                        )
                    }
                }
                Some("question.asked") => (
                    ProviderRequestKind::UserInput,
                    "OpenCode needs your answer".to_owned(),
                    String::new(),
                    ProviderRequestPrompt::Answers {
                        questions: questions(properties)?,
                    },
                    format!("/question/{id}/reply"),
                    ReplyShape::V1Question,
                ),
                Some("form.created") if v2 && api == OpenCodeApi::V2 => {
                    let form = properties.get("form").ok_or_else(protocol_error)?;
                    let title = text(form, "title")?.to_owned();
                    let session_id =
                        clean_session_id(text(form, "sessionID")?).ok_or_else(protocol_error)?;
                    let (questions, fields) = form_fields(form)?;
                    (
                        ProviderRequestKind::UserInput,
                        title,
                        String::new(),
                        ProviderRequestPrompt::Answers { questions },
                        format!("/api/session/{session_id}/form/{id}/reply"),
                        ReplyShape::V2Form(fields),
                    )
                }
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
            path,
            reply,
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
            ) => Ok(match &self.reply {
                ReplyShape::V2Permission => json!({"decision": option_id}),
                ReplyShape::V1Permission | ReplyShape::V1Question | ReplyShape::V2Form(_) => {
                    json!({"reply": option_id})
                }
            }),
            (
                ProviderRequestPrompt::Answers { questions },
                ProviderRequestResolution::Answers { answers },
            ) => {
                if let ReplyShape::V2Form(fields) = &self.reply {
                    return form_answer(fields, answers);
                }
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

fn form_fields(
    form: &Value,
) -> Result<(Vec<ProviderRequestQuestion>, Vec<FormField>), DriverError> {
    let fields = form
        .get("fields")
        .and_then(Value::as_array)
        .ok_or_else(protocol_error)?;
    let mut questions = Vec::new();
    let mut mapped = Vec::new();
    for field in fields {
        let key = text(field, "key")?.to_owned();
        if key.is_empty() || key.len() > 128 || key.chars().any(char::is_control) {
            return Err(protocol_error());
        }
        let field_type = text(field, "type")?;
        let title = optional_text(field, "title")?;
        let description = optional_text(field, "description")?;
        let (kind, choices, options, multiple, other) = match field_type {
            "string" => {
                let choices = form_choices(field)?;
                let options = form_options(&choices);
                (
                    FormFieldKind::Text,
                    choices,
                    options,
                    false,
                    boolean(field, "custom", field.get("options").is_none())?,
                )
            }
            "number" => (FormFieldKind::Number, Vec::new(), Vec::new(), false, true),
            "integer" => (FormFieldKind::Integer, Vec::new(), Vec::new(), false, true),
            "boolean" => {
                let choices = vec![
                    ("true".to_owned(), "true".to_owned()),
                    ("false".to_owned(), "false".to_owned()),
                ];
                let options = form_options(&choices);
                (FormFieldKind::Boolean, choices, options, false, false)
            }
            "multiselect" => {
                let choices = form_choices(field)?;
                let options = form_options(&choices);
                (
                    FormFieldKind::Strings,
                    choices,
                    options,
                    true,
                    boolean(field, "custom", false)?,
                )
            }
            // External fields cannot be answered inline; the owner can still supply text.
            "external" => (FormFieldKind::Text, Vec::new(), Vec::new(), false, true),
            _ => return Err(protocol_error()),
        };
        questions.push(ProviderRequestQuestion {
            id: key.clone(),
            header: title.to_owned(),
            question: if description.is_empty() {
                title.to_owned()
            } else {
                description.to_owned()
            },
            options,
            multiple,
            is_other: other,
            is_secret: false,
        });
        mapped.push(FormField { key, kind, choices });
    }
    Ok((questions, mapped))
}

fn form_choices(field: &Value) -> Result<Vec<(String, String)>, DriverError> {
    match field.get("options").filter(|value| !value.is_null()) {
        Some(options) => options
            .as_array()
            .ok_or_else(protocol_error)?
            .iter()
            .map(|option| {
                Ok((
                    text(option, "label")?.to_owned(),
                    text(option, "value")?.to_owned(),
                ))
            })
            .collect(),
        None => Ok(Vec::new()),
    }
}

fn form_options(choices: &[(String, String)]) -> Vec<ProviderRequestOption> {
    choices
        .iter()
        .map(|(label, value)| option(value, label, "answer", ""))
        .collect()
}

fn form_answer(
    fields: &[FormField],
    answers: &std::collections::BTreeMap<String, Vec<String>>,
) -> Result<Value, DriverError> {
    let mut answer = serde_json::Map::new();
    for field in fields {
        let values = answers.get(&field.key).ok_or_else(protocol_error)?;
        let value = match field.kind {
            FormFieldKind::Text => json!(choice_value(&field.choices, single(values)?)),
            FormFieldKind::Number => {
                let value = single(values)?
                    .parse::<f64>()
                    .map_err(|_| protocol_error())?;
                json!(serde_json::Number::from_f64(value).ok_or_else(protocol_error)?)
            }
            FormFieldKind::Integer => json!(
                single(values)?
                    .parse::<i64>()
                    .map_err(|_| protocol_error())?
            ),
            FormFieldKind::Boolean => json!(match single(values)?.to_ascii_lowercase().as_str() {
                "true" | "yes" | "1" => true,
                "false" | "no" | "0" => false,
                _ => return Err(protocol_error()),
            }),
            FormFieldKind::Strings => json!(
                values
                    .iter()
                    .map(|value| choice_value(&field.choices, value))
                    .collect::<Vec<_>>()
            ),
        };
        answer.insert(field.key.clone(), value);
    }
    Ok(json!({"answer": answer}))
}

fn single(values: &[String]) -> Result<&str, DriverError> {
    match values {
        [value] => Ok(value),
        _ => Err(protocol_error()),
    }
}

fn choice_value(choices: &[(String, String)], answer: &str) -> String {
    choices
        .iter()
        .find(|(label, _)| label == answer)
        .map_or_else(|| answer.to_owned(), |(_, value)| value.clone())
}

fn request_id(properties: &Value) -> Result<String, DriverError> {
    let id = properties
        .get("id")
        .or_else(|| properties.get("requestID"))
        .or_else(|| properties.pointer("/form/id"))
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
        let mapped = NativeRequest::parse(
            &json!({"type": "question.asked", "properties": {
                "id": "q-1", "questions": [
                    {"question": "Pick two", "multiple": true, "custom": false, "options": [{"label":"A"}, {"label":"B"}]},
                    {"question": "Enter text", "custom": true}
                ]
            }}),
            OpenCodeApi::V1,
        )?;
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
        let v2_permission = NativeRequest::parse(
            &json!({"type": "permission.asked", "data": {
                "sessionID": "ses_1", "id": "p-2", "action": "bash",
                "resources": ["echo fixture"]
            }}),
            OpenCodeApi::V2,
        )?;
        assert_eq!(
            v2_permission.path,
            "/api/session/ses_1/permission/p-2/reply"
        );
        assert_eq!(
            v2_permission.response(&ProviderRequestResolution::Option {
                option_id: "once".to_owned()
            })?,
            json!({"decision": "once"})
        );
        let v2_form = NativeRequest::parse(
            &json!({"type": "form.created", "data": {"form": {
                "id": "frm_1", "sessionID": "ses_1", "title": "Pick things",
                "fields": [
                    {"key": "color", "title": "Color", "type": "string",
                     "options": [{"value": "red", "label": "Red"}], "custom": true},
                    {"key": "tags", "title": "Tags", "type": "multiselect",
                     "options": [{"value": "a", "label": "A"}, {"value": "b", "label": "B"}],
                     "custom": false},
                    {"key": "ok", "title": "OK", "type": "boolean"}
                ]
            }}}),
            OpenCodeApi::V2,
        )?;
        assert_eq!(v2_form.path, "/api/session/ses_1/form/frm_1/reply");
        assert_eq!(
            v2_form.response(&ProviderRequestResolution::Answers {
                answers: std::collections::BTreeMap::from([
                    ("color".to_owned(), vec!["Red".to_owned()]),
                    ("tags".to_owned(), vec!["A".to_owned(), "B".to_owned()]),
                    ("ok".to_owned(), vec!["true".to_owned()]),
                ])
            })?,
            json!({"answer": {"color": "red", "tags": ["a", "b"], "ok": true}})
        );
        let permission = NativeRequest::parse(
            &json!({"type": "permission.asked", "properties": {"id": "p-1", "permission":"bash", "patterns":[], "always":["*"]}}),
            OpenCodeApi::V1,
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
                &json!({"type":"permission.asked", "properties":{"id":"p-1", "requestID":"p-2"}}),
                OpenCodeApi::V1,
            )
            .is_err()
        );
        Ok(())
    }
}
