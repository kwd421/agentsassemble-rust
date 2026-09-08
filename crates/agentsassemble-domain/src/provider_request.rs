//! Owner-facing provider requests; native envelopes and secret delivery stay outside history.
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ProviderRequestKind {
    Permission,
    UserInput,
    ExternalAction,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct ProviderRequestOption {
    pub id: String,
    pub label: String,
    pub kind: String,
    pub description: String,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct ProviderRequestQuestion {
    pub id: String,
    pub header: String,
    pub question: String,
    pub options: Vec<ProviderRequestOption>,
    pub multiple: bool,
    pub is_other: bool,
    pub is_secret: bool,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "response_kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProviderRequestPrompt {
    Option {
        options: Vec<ProviderRequestOption>,
    },
    Answers {
        questions: Vec<ProviderRequestQuestion>,
    },
    Acknowledge {
        action_url: Option<String>,
    },
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct ProviderRequest {
    pub provider_request_id: Uuid,
    pub request_kind: ProviderRequestKind,
    pub title: String,
    pub description: String,
    pub timeout_seconds: u16,
    pub prompt: ProviderRequestPrompt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum PendingProviderRequestState {
    Open,
    Resolving,
}

/// Owner-only current state, independent of the bounded room event window.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct PendingProviderRequest {
    pub session_id: String,
    pub request: ProviderRequest,
    pub expires_at: chrono::DateTime<chrono::Utc>,
    pub state: PendingProviderRequestState,
}

impl std::fmt::Debug for PendingProviderRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PendingProviderRequest")
            .field("session_id", &self.session_id)
            .field("provider_request_id", &self.request.provider_request_id)
            .field("state", &self.state)
            .finish_non_exhaustive()
    }
}

/// Deliberately has no Debug: answers can contain credentials and are live delivery only.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "response_kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProviderRequestResolution {
    Option {
        option_id: String,
    },
    Answers {
        answers: BTreeMap<String, Vec<String>>,
    },
    Acknowledge,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "response_kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DurableProviderResolution {
    Option {
        option_id: String,
    },
    Answers {
        answers: BTreeMap<String, Vec<String>>,
        secret_answered_question_ids: Vec<String>,
    },
    Acknowledge,
}

impl ProviderRequest {
    #[must_use]
    pub fn is_valid(&self) -> bool {
        !self.provider_request_id.is_nil()
            && required_text(&self.title, 160)
            && text(&self.description, 1200)
            && (15..=900).contains(&self.timeout_seconds)
            && match (&self.request_kind, &self.prompt) {
                (ProviderRequestKind::Permission, ProviderRequestPrompt::Option { options }) => {
                    !options.is_empty() && valid_options(options)
                }
                (ProviderRequestKind::UserInput, ProviderRequestPrompt::Answers { questions }) => {
                    (1..=3).contains(&questions.len())
                        && unique(questions.iter().map(|question| question.id.as_str()))
                        && questions.iter().all(|question| {
                            required_text(&question.id, 128)
                                && text(&question.header, 120)
                                && required_text(&question.question, 800)
                                && valid_options(&question.options)
                                && unique(
                                    question.options.iter().map(|option| option.label.as_str()),
                                )
                        })
                }
                (
                    ProviderRequestKind::ExternalAction,
                    ProviderRequestPrompt::Acknowledge { action_url },
                ) => action_url.as_ref().is_none_or(|action_url| {
                    required_text(action_url, 2000)
                        && url::Url::parse(action_url).is_ok_and(|url| {
                            url.scheme() == "https"
                                && url.host_str().is_some()
                                && url.username().is_empty()
                                && url.password().is_none()
                        })
                }),
                _ => false,
            }
    }

    /// Validates the provider's offered choices before creating any durable resolution.
    #[must_use]
    pub fn durable_resolution(
        &self,
        resolution: &ProviderRequestResolution,
    ) -> Option<DurableProviderResolution> {
        if !self.is_valid() {
            return None;
        }
        match (&self.prompt, resolution) {
            (
                ProviderRequestPrompt::Option { options },
                ProviderRequestResolution::Option { option_id },
            ) => options
                .iter()
                .any(|option| option.id == *option_id)
                .then(|| DurableProviderResolution::Option {
                    option_id: option_id.clone(),
                }),
            (
                ProviderRequestPrompt::Answers { questions },
                ProviderRequestResolution::Answers { answers },
            ) => {
                if answers.len() != questions.len()
                    || !questions.iter().all(|question| {
                        answers.get(&question.id).is_some_and(|values| {
                            (1..=if question.multiple { 4 } else { 1 }).contains(&values.len())
                                && unique(values.iter().map(String::as_str))
                                && values.iter().all(|value| {
                                    required_text(value, 1000)
                                        && (question.is_other
                                            || question.options.is_empty()
                                            || question
                                                .options
                                                .iter()
                                                .any(|option| option.label == *value))
                                })
                        })
                    })
                {
                    return None;
                }
                let secret: BTreeSet<&str> = questions
                    .iter()
                    .filter(|question| question.is_secret)
                    .map(|question| question.id.as_str())
                    .collect();
                Some(DurableProviderResolution::Answers {
                    answers: answers
                        .iter()
                        .filter(|(id, _)| !secret.contains(id.as_str()))
                        .map(|(id, values)| (id.clone(), values.clone()))
                        .collect(),
                    secret_answered_question_ids: secret.into_iter().map(str::to_owned).collect(),
                })
            }
            (ProviderRequestPrompt::Acknowledge { .. }, ProviderRequestResolution::Acknowledge) => {
                Some(DurableProviderResolution::Acknowledge)
            }
            _ => None,
        }
    }
}

fn valid_options(options: &[ProviderRequestOption]) -> bool {
    options.len() <= 12
        && unique(options.iter().map(|option| option.id.as_str()))
        && options.iter().all(|option| {
            required_text(&option.id, 128)
                && required_text(&option.label, 240)
                && text(&option.kind, 64)
                && text(&option.description, 400)
        })
}

fn unique<'a>(values: impl Iterator<Item = &'a str>) -> bool {
    let mut seen = BTreeSet::new();
    values.into_iter().all(|value| seen.insert(value))
}

fn required_text(value: &str, limit: usize) -> bool {
    !value.trim().is_empty() && text(value, limit)
}
fn text(value: &str, limit: usize) -> bool {
    value.chars().count() <= limit && !value.chars().any(char::is_control)
}

#[cfg(test)]
#[path = "provider_request_tests.rs"]
mod tests;
