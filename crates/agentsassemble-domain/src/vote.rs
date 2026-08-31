use std::collections::{BTreeMap, BTreeSet};

use caseless::default_case_fold_str;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use ts_rs::TS;

use crate::{
    AuthenticatedPrincipal, CommandRejection, Participant, RoomEvent, clean_identifier,
    clean_message,
    command::{parse_attachment_ids, prepare_participant_message_event},
    has_visible_text, is_message_event_id,
};

pub const MIN_VOTE_DURATION_SECONDS: u32 = 30;
pub const MAX_VOTE_DURATION_SECONDS: u32 = 86_400;
pub const VOTE_QUESTION_CHARACTER_LIMIT: usize = 300;
pub const VOTE_OPTION_CHARACTER_LIMIT: usize = 100;
pub const MIN_VOTE_OPTIONS: usize = 2;
pub const MAX_VOTE_OPTIONS: usize = 10;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoteCreate {
    pub question: String,
    pub options: Vec<String>,
    pub duration_seconds: u32,
    pub attachment_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoteCast {
    pub vote_id: String,
    pub choice: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoteReference {
    pub vote_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VoteCommand {
    Create(VoteCreate),
    Cast(VoteCast),
    Withdraw(VoteReference),
    Close(VoteReference),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct VoteSummary {
    pub vote_id: String,
    pub question: String,
    pub options: Vec<String>,
    pub vote_duration_seconds: u32,
    pub vote_deadline_at: String,
    pub created_by: String,
    pub created_at: String,
    pub tallies: std::collections::BTreeMap<String, u64>,
    pub own_choice: String,
    pub total_votes: u64,
    pub closed: bool,
    pub closed_at: String,
    pub close_reason: String,
}

impl VoteCommand {
    /// Parses one exact non-ordinary `message.send` vote payload.
    ///
    /// # Errors
    ///
    /// Rejects unknown fields, unsupported kinds, invalid identifiers, and vote
    /// definitions outside the current product bounds.
    pub fn from_payload(payload: &Value) -> Result<Self, CommandRejection> {
        let object = payload
            .as_object()
            .ok_or_else(|| CommandRejection::new("bad_request", "payload must be an object."))?;
        let kind = object.get("kind").and_then(Value::as_str).ok_or_else(|| {
            CommandRejection::new("bad_request", "A canonical vote kind is required.")
        })?;
        match kind {
            "vote" => parse_create(object).map(Self::Create),
            "vote_cast" => parse_cast(object).map(Self::Cast),
            "vote_withdraw" => parse_reference(object, "vote_withdraw").map(Self::Withdraw),
            "vote_close" => parse_reference(object, "vote_close").map(Self::Close),
            _ => Err(CommandRejection::new(
                "invalid_message_kind",
                "Room message kind is unsupported.",
            )),
        }
    }

    #[must_use]
    pub const fn message_kind(&self) -> &'static str {
        match self {
            Self::Create(_) => "vote",
            Self::Cast(_) => "vote_cast",
            Self::Withdraw(_) => "vote_withdraw",
            Self::Close(_) => "vote_close",
        }
    }
}

impl VoteReference {
    /// Parses the exact read-only `room.vote.summary` payload.
    ///
    /// # Errors
    ///
    /// Rejects non-object payloads, unknown fields, missing identifiers, and aliases that are not
    /// already canonical vote IDs.
    pub fn from_summary_payload(payload: &Value) -> Result<Self, CommandRejection> {
        let object = payload
            .as_object()
            .ok_or_else(|| CommandRejection::new("bad_request", "payload must be an object."))?;
        if object.len() != 1 || !object.contains_key("vote_id") {
            return Err(CommandRejection::new(
                "bad_request",
                "room.vote.summary accepts exactly vote_id.",
            ));
        }
        Ok(Self {
            vote_id: parse_vote_id(object.get("vote_id"))?,
        })
    }
}

/// Validates one canonical vote identifier without accepting normalized aliases.
///
/// # Errors
///
/// Rejects empty, oversized, NUL-containing, or non-canonical identifiers.
pub fn validate_vote_id(value: &str) -> Result<String, CommandRejection> {
    let canonical = clean_identifier(value, 128);
    if canonical != value || !is_message_event_id(&canonical) {
        return Err(CommandRejection::new("invalid_vote", "vote_id is invalid."));
    }
    Ok(canonical)
}

/// Resolves a ballot by case-insensitive option text or one-based option number.
#[must_use]
pub fn resolve_vote_choice(choice: &str, options: &[String]) -> Option<String> {
    let choice = clean_message(choice, 200);
    if !has_visible_text(&choice) {
        return None;
    }
    let folded = default_case_fold_str(&choice);
    if let Some(option) = options
        .iter()
        .find(|option| default_case_fold_str(option) == folded)
    {
        return Some(option.clone());
    }
    let index = choice.parse::<usize>().ok()?;
    index
        .checked_sub(1)
        .and_then(|index| options.get(index))
        .cloned()
}

#[must_use]
pub fn vote_deadline_at(now: DateTime<Utc>, duration_seconds: u32) -> Option<DateTime<Utc>> {
    (duration_seconds > 0).then(|| now + Duration::seconds(i64::from(duration_seconds)))
}

/// Builds one canonical poll or ballot event after any poll-state checks.
///
/// A cast's choice must already be resolved against the stored poll options by
/// the persistence owner. This function owns only authenticated speech identity
/// and the public event representation.
///
/// # Errors
///
/// Rejects stale/read-only/muted speech authority or an invalid room sequence.
pub fn prepare_vote_event(
    principal: &AuthenticatedPrincipal,
    participant: &Participant,
    command: &VoteCommand,
    sequence: i64,
    now: DateTime<Utc>,
) -> Result<RoomEvent, CommandRejection> {
    let mut extra = BTreeMap::new();
    match command {
        VoteCommand::Create(create) => {
            extra.insert("vote_question".to_owned(), json!(create.question));
            extra.insert("vote_options".to_owned(), json!(create.options));
            extra.insert(
                "vote_duration_seconds".to_owned(),
                json!(create.duration_seconds),
            );
            extra.insert(
                "vote_deadline_at".to_owned(),
                json!(
                    vote_deadline_at(now, create.duration_seconds)
                        .map(|deadline| deadline.to_rfc3339())
                        .unwrap_or_default()
                ),
            );
        }
        VoteCommand::Cast(cast) => {
            extra.insert("vote_id".to_owned(), json!(cast.vote_id));
            extra.insert("vote_choice".to_owned(), json!(cast.choice));
        }
        VoteCommand::Withdraw(reference) | VoteCommand::Close(reference) => {
            extra.insert("vote_id".to_owned(), json!(reference.vote_id));
        }
    }
    prepare_participant_message_event(
        principal,
        participant,
        sequence,
        now,
        String::new(),
        command.message_kind(),
        extra,
    )
}

fn parse_create(object: &Map<String, Value>) -> Result<VoteCreate, CommandRejection> {
    require_exact_keys(
        object,
        &["kind", "vote_question", "vote_options"],
        &["vote_duration_seconds", "attachment_ids"],
        "vote",
    )?;
    let question = object
        .get("vote_question")
        .and_then(Value::as_str)
        .map(|value| clean_message(value, VOTE_QUESTION_CHARACTER_LIMIT))
        .filter(|value| has_visible_text(value))
        .ok_or_else(|| CommandRejection::new("invalid_vote", "vote_question is required."))?;
    let values = object
        .get("vote_options")
        .and_then(Value::as_array)
        .ok_or_else(|| CommandRejection::new("invalid_vote", "vote_options must be an array."))?;
    let mut seen = BTreeSet::new();
    let mut options = Vec::with_capacity(values.len().min(MAX_VOTE_OPTIONS));
    for value in values {
        let value = value.as_str().ok_or_else(|| {
            CommandRejection::new("invalid_vote", "Every vote option must be text.")
        })?;
        let option = clean_message(value, VOTE_OPTION_CHARACTER_LIMIT);
        if !has_visible_text(&option) || !seen.insert(default_case_fold_str(&option)) {
            continue;
        }
        options.push(option);
        if options.len() > MAX_VOTE_OPTIONS {
            return Err(CommandRejection::new(
                "invalid_vote",
                format!("A vote supports at most {MAX_VOTE_OPTIONS} options."),
            ));
        }
    }
    if options.len() < MIN_VOTE_OPTIONS {
        return Err(CommandRejection::new(
            "invalid_vote",
            "A vote requires at least two distinct options.",
        ));
    }
    let duration_seconds = match object.get("vote_duration_seconds") {
        None => 0,
        Some(Value::Number(value)) => value
            .as_u64()
            .and_then(|value| u32::try_from(value).ok())
            .filter(|value| {
                *value == 0
                    || (MIN_VOTE_DURATION_SECONDS..=MAX_VOTE_DURATION_SECONDS).contains(value)
            })
            .ok_or_else(invalid_vote_duration)?,
        Some(_) => return Err(invalid_vote_duration()),
    };
    let attachment_ids = parse_attachment_ids(object.get("attachment_ids"))?;
    Ok(VoteCreate {
        question,
        options,
        duration_seconds,
        attachment_ids,
    })
}

fn parse_cast(object: &Map<String, Value>) -> Result<VoteCast, CommandRejection> {
    require_exact_keys(
        object,
        &["kind", "vote_id", "vote_choice"],
        &[],
        "vote_cast",
    )?;
    let vote_id = parse_vote_id(object.get("vote_id"))?;
    let choice = object
        .get("vote_choice")
        .and_then(Value::as_str)
        .map(|value| clean_message(value, 200))
        .filter(|value| has_visible_text(value))
        .ok_or_else(|| CommandRejection::new("invalid_vote_choice", "vote_choice is required."))?;
    Ok(VoteCast { vote_id, choice })
}

fn parse_reference(
    object: &Map<String, Value>,
    kind: &'static str,
) -> Result<VoteReference, CommandRejection> {
    require_exact_keys(object, &["kind", "vote_id"], &[], kind)?;
    Ok(VoteReference {
        vote_id: parse_vote_id(object.get("vote_id"))?,
    })
}

fn parse_vote_id(value: Option<&Value>) -> Result<String, CommandRejection> {
    let value = value
        .and_then(Value::as_str)
        .ok_or_else(|| CommandRejection::new("invalid_vote", "vote_id must be a string."))?;
    validate_vote_id(value)
}

fn require_exact_keys(
    object: &Map<String, Value>,
    required: &[&str],
    optional: &[&str],
    kind: &'static str,
) -> Result<(), CommandRejection> {
    if required.iter().any(|key| !object.contains_key(*key))
        || object
            .keys()
            .any(|key| !required.contains(&key.as_str()) && !optional.contains(&key.as_str()))
    {
        return Err(CommandRejection::new(
            "bad_request",
            format!("message.send {kind} payload has an invalid shape."),
        ));
    }
    Ok(())
}

fn invalid_vote_duration() -> CommandRejection {
    CommandRejection::new(
        "invalid_vote_duration",
        format!(
            "vote_duration_seconds must be 0 or between {MIN_VOTE_DURATION_SECONDS} and {MAX_VOTE_DURATION_SECONDS}."
        ),
    )
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use serde_json::json;

    use crate::{
        AuthenticatedPrincipal, CapabilitySet, ClientKind, InviteScope, Participant,
        ParticipantRole, ParticipantStatus,
    };

    use super::{
        VoteCommand, VoteReference, prepare_vote_event, resolve_vote_choice, validate_vote_id,
    };

    #[test]
    fn create_normalizes_one_exact_bounded_definition() {
        let command = VoteCommand::from_payload(&json!({
            "kind": "vote",
            "vote_question": "  Ship\r\nthis?  ",
            "vote_options": [" Yes ", "YES", "", "No"],
            "vote_duration_seconds": 30,
            "attachment_ids": ["ma_0123456789abcdef0123456789abcdef"]
        }))
        .unwrap_or_else(|error| panic!("vote create: {error}"));
        let VoteCommand::Create(command) = command else {
            panic!("expected create")
        };
        assert_eq!(command.question, "Ship\nthis?");
        assert_eq!(command.options, ["Yes", "No"]);
        assert_eq!(command.duration_seconds, 30);
        assert_eq!(command.attachment_ids.len(), 1);
    }

    #[test]
    fn variants_reject_ambiguous_shapes_and_bounds() {
        for payload in [
            json!({"kind": "message", "content": "not a vote"}),
            json!({"kind": "vote", "vote_question": "?", "vote_options": ["same", "SAME"]}),
            json!({"kind": "vote", "vote_question": "?", "vote_options": ["a", "b"], "vote_duration_seconds": 29}),
            json!({"kind": "vote", "vote_question": "?", "vote_options": ["a", "b"], "vote_duration_seconds": null}),
            json!({"kind": "vote_cast", "vote_id": " poll ", "vote_choice": "a"}),
            json!({"kind": "vote_cast", "vote_id": "poll", "vote_choice": "a", "content": ""}),
            json!({"kind": "vote_withdraw", "vote_id": "poll", "vote_choice": "a"}),
            json!({"kind": "vote_close", "vote_id": "poll", "extra": true}),
        ] {
            assert!(
                VoteCommand::from_payload(&payload).is_err(),
                "accepted {payload}"
            );
        }
    }

    #[test]
    fn cast_and_references_preserve_canonical_ids() {
        assert_eq!(validate_vote_id("poll-1").as_deref(), Ok("poll-1"));
        assert!(validate_vote_id(" poll-1 ").is_err());
        assert_eq!(
            VoteCommand::from_payload(
                &json!({"kind": "vote_cast", "vote_id": "poll-1", "vote_choice": " 2 "})
            )
            .unwrap_or_else(|error| panic!("vote cast: {error}"))
            .message_kind(),
            "vote_cast"
        );
        assert_eq!(
            VoteCommand::from_payload(&json!({"kind": "vote_withdraw", "vote_id": "poll-1"}))
                .unwrap_or_else(|error| panic!("vote withdraw: {error}"))
                .message_kind(),
            "vote_withdraw"
        );
        assert_eq!(
            VoteCommand::from_payload(&json!({"kind": "vote_close", "vote_id": "poll-1"}))
                .unwrap_or_else(|error| panic!("vote close: {error}"))
                .message_kind(),
            "vote_close"
        );
        assert_eq!(
            VoteReference::from_summary_payload(&json!({"vote_id": "poll-1"}))
                .map(|reference| reference.vote_id),
            Ok("poll-1".to_owned())
        );
        assert!(
            VoteReference::from_summary_payload(&json!({
                "vote_id": "poll-1",
                "room_id": "other"
            }))
            .is_err()
        );
    }

    #[test]
    fn choice_resolution_matches_text_or_one_based_number() {
        let options = vec!["Straße".to_owned(), "North".to_owned()];
        assert_eq!(
            resolve_vote_choice("STRASSE", &options).as_deref(),
            Some("Straße")
        );
        assert_eq!(resolve_vote_choice("2", &options).as_deref(), Some("North"));
        assert_eq!(resolve_vote_choice("0", &options), None);
        assert_eq!(resolve_vote_choice("unknown", &options), None);
    }

    #[test]
    fn vote_event_uses_authenticated_actor_and_canonical_deadline() {
        let now = Utc
            .with_ymd_and_hms(2026, 8, 31, 0, 0, 0)
            .single()
            .unwrap_or_else(|| panic!("valid fixture time"));
        let principal = AuthenticatedPrincipal {
            principal_id: "human-user".to_owned(),
            participant_id: "human-participant".to_owned(),
            display_name: "Untrusted Copy".to_owned(),
            room_id: "general".to_owned(),
            client_kind: ClientKind::Browser,
            invite_scope: InviteScope::ReadWrite,
            is_operator: false,
            capabilities: CapabilitySet::for_principal(
                ClientKind::Browser,
                InviteScope::ReadWrite,
                false,
            ),
        };
        let participant = Participant {
            room_id: "general".to_owned(),
            participant_id: "human-participant".to_owned(),
            display_name: "Stored Human".to_owned(),
            avatar_image_url: String::new(),
            participant_type: "human".to_owned(),
            status: ParticipantStatus::Joined,
            role: ParticipantRole::Human,
            owner_id: "human-user".to_owned(),
            muted: false,
            created_at: now,
            updated_at: now,
        };
        let command = VoteCommand::from_payload(&json!({
            "kind": "vote",
            "vote_question": "Ship?",
            "vote_options": ["Yes", "No"],
            "vote_duration_seconds": 30
        }))
        .unwrap_or_else(|error| panic!("vote create: {error}"));
        let event = prepare_vote_event(&principal, &participant, &command, 7, now)
            .unwrap_or_else(|error| panic!("vote event: {error}"));
        assert_eq!(event.actor.participant_id, "human-participant");
        assert_eq!(event.display_name.as_deref(), Some("Stored Human"));
        assert_eq!(event.message_kind.as_deref(), Some("vote"));
        assert_eq!(event.extra["vote_question"], json!("Ship?"));
        assert_eq!(
            event.extra["vote_deadline_at"],
            json!("2026-08-31T00:00:30+00:00")
        );
    }
}
