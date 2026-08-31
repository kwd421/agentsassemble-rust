use agentsassemble_domain::{
    MAX_MESSAGE_SEARCH_CURSOR_BYTES, RoomRandomRequest, VoteCommand, clean_message_search_query,
    is_message_attachment_id, is_message_event_id,
};
use serde_json::{Value, json};

use crate::{
    room_portal::{CAST_VOTE_TOOL, CLOSE_VOTE_TOOL, CREATE_VOTE_TOOL, WITHDRAW_VOTE_TOOL},
    room_portal_tool_contract::CreateVote,
};

pub(super) fn helper_tool(
    action: &str,
    mut arguments: impl Iterator<Item = String>,
) -> Result<(&'static str, Value), &'static str> {
    if action.starts_with("vote-") {
        return vote_tool(action, arguments);
    }
    let (tool, payload) = match action {
        "read" if arguments.next().is_none() => ("read_discussion", json!({})),
        "media" => {
            let attachment_id = arguments
                .next()
                .filter(|value| is_message_attachment_id(value))
                .ok_or("usage: agentsassemble-room media <attachment-id>")?;
            if arguments.next().is_some() {
                return Err("usage: agentsassemble-room media <attachment-id>");
            }
            ("read_attachment", json!({"attachment_id": attachment_id}))
        }
        "search" => {
            let query = arguments
                .next()
                .map(|value| clean_message_search_query(&value))
                .filter(|value| !value.is_empty())
                .ok_or("usage: agentsassemble-room search <query> [cursor]")?;
            let cursor = arguments.next().unwrap_or_default();
            if cursor.len() > MAX_MESSAGE_SEARCH_CURSOR_BYTES || arguments.next().is_some() {
                return Err("usage: agentsassemble-room search <query> [cursor]");
            }
            ("search_messages", json!({"query": query, "cursor": cursor}))
        }
        "context" => {
            let event_id = arguments
                .next()
                .filter(|value| is_message_event_id(value))
                .ok_or("usage: agentsassemble-room context <event-id>")?;
            if arguments.next().is_some() {
                return Err("usage: agentsassemble-room context <event-id>");
            }
            ("read_message_context", json!({"event_id": event_id}))
        }
        "speak" => {
            let content = arguments.collect::<Vec<_>>().join(" ").trim().to_owned();
            if content.is_empty() {
                return Err("usage: agentsassemble-room speak <message>");
            }
            (
                "publish_message",
                json!({"content": content, "next_agent_id": ""}),
            )
        }
        "speak-to" => {
            let target = arguments
                .next()
                .filter(|value| valid_agent_id(value))
                .ok_or("usage: agentsassemble-room speak-to <agent-id> <message>")?;
            let content = arguments.collect::<Vec<_>>().join(" ").trim().to_owned();
            if content.is_empty() {
                return Err("usage: agentsassemble-room speak-to <agent-id> <message>");
            }
            (
                "publish_message",
                json!({"content": content, "next_agent_id": target}),
            )
        }
        "decline" => {
            let reason = arguments
                .next()
                .filter(|value| {
                    matches!(
                        value.as_str(),
                        "nothing_useful_to_add" | "not_addressed" | "duplicate"
                    )
                })
                .ok_or("usage: agentsassemble-room decline <reason>")?;
            if arguments.next().is_some() {
                return Err("usage: agentsassemble-room decline <reason>");
            }
            ("decline_to_speak", json!({"reason_code": reason}))
        }
        "roll" => {
            let notation = arguments
                .next()
                .ok_or("usage: agentsassemble-room roll <NdS+M>")?;
            if arguments.next().is_some() {
                return Err("usage: agentsassemble-room roll <NdS+M>");
            }
            ("roll_dice", json!({"notation": notation, "reason": ""}))
        }
        "choose" => {
            let encoded = arguments
                .next()
                .ok_or("usage: agentsassemble-room choose <json-options>")?;
            if arguments.next().is_some() {
                return Err("usage: agentsassemble-room choose <json-options>");
            }
            let options: Value = serde_json::from_str(&encoded)
                .map_err(|_| "random options must be a JSON array")?;
            ("choose_random", json!({"options": options, "reason": ""}))
        }
        _ => return Err("unsupported room helper command"),
    };
    Ok((tool, payload))
}

fn vote_tool(
    action: &str,
    mut arguments: impl Iterator<Item = String>,
) -> Result<(&'static str, Value), &'static str> {
    match action {
        "vote-create" => {
            let encoded = one_argument(
                &mut arguments,
                "usage: agentsassemble-room vote-create <json-definition>",
            )?;
            let input = serde_json::from_str::<CreateVote>(&encoded)
                .map_err(|_| "vote definition must be one exact JSON object")?;
            let domain_payload = json!({
                "kind": "vote",
                "vote_question": input.question,
                "vote_options": input.options,
                "vote_duration_seconds": input.duration_seconds,
            });
            validate_vote(&domain_payload)?;
            Ok((
                CREATE_VOTE_TOOL,
                json!({
                    "question": domain_payload["vote_question"],
                    "options": domain_payload["vote_options"],
                    "duration_seconds": domain_payload["vote_duration_seconds"],
                }),
            ))
        }
        "vote-cast" => {
            let usage = "usage: agentsassemble-room vote-cast <vote-id> <choice>";
            let vote_id = arguments.next().ok_or(usage)?;
            let choice = arguments.next().ok_or(usage)?;
            if arguments.next().is_some() {
                return Err(usage);
            }
            validate_vote(&json!({
                "kind": "vote_cast",
                "vote_id": vote_id,
                "vote_choice": choice,
            }))?;
            Ok((
                CAST_VOTE_TOOL,
                json!({"vote_id": vote_id, "choice": choice}),
            ))
        }
        "vote-withdraw" | "vote-close" => {
            let (usage, kind, tool) = if action == "vote-withdraw" {
                (
                    "usage: agentsassemble-room vote-withdraw <vote-id>",
                    "vote_withdraw",
                    WITHDRAW_VOTE_TOOL,
                )
            } else {
                (
                    "usage: agentsassemble-room vote-close <vote-id>",
                    "vote_close",
                    CLOSE_VOTE_TOOL,
                )
            };
            let vote_id = one_argument(&mut arguments, usage)?;
            validate_vote(&json!({"kind": kind, "vote_id": vote_id}))?;
            Ok((tool, json!({"vote_id": vote_id})))
        }
        _ => Err("unsupported room helper command"),
    }
}

fn one_argument(
    arguments: &mut impl Iterator<Item = String>,
    usage: &'static str,
) -> Result<String, &'static str> {
    let value = arguments.next().ok_or(usage)?;
    if arguments.next().is_some() {
        return Err(usage);
    }
    Ok(value)
}

fn validate_vote(payload: &Value) -> Result<(), &'static str> {
    VoteCommand::from_payload(payload)
        .map(|_| ())
        .map_err(|_| "room vote action is invalid")
}

pub(crate) fn safe_room_command(command: &str, command_prefix: &str) -> bool {
    let Some(arguments) = command
        .strip_prefix(command_prefix)
        .and_then(|suffix| suffix.strip_prefix(' '))
    else {
        return false;
    };
    if arguments.contains(['\r', '\n']) || unsafe_shell_arguments(arguments) {
        return false;
    }
    let Some(parts) = shlex::split(arguments) else {
        return false;
    };
    if parts.is_empty() {
        return false;
    }
    match parts[0].as_str() {
        "help" | "read" => parts.len() == 1,
        "media" => parts.len() == 2 && is_message_attachment_id(&parts[1]),
        "search" | "context" | "vote-create" | "vote-cast" | "vote-withdraw" | "vote-close" => {
            helper_tool(parts[0].as_str(), parts[1..].iter().cloned()).is_ok()
        }
        "decline" => {
            parts.len() == 2
                && matches!(
                    parts[1].as_str(),
                    "nothing_useful_to_add" | "not_addressed" | "duplicate"
                )
        }
        "speak" => parts.len() >= 2,
        "speak-to" => parts.len() >= 3 && valid_agent_id(&parts[1]),
        "roll" => {
            parts.len() == 2
                && RoomRandomRequest::parse(
                    "room.random.roll",
                    &json!({"notation": parts[1], "reason": ""}),
                )
                .is_ok()
        }
        "choose" => {
            parts.len() == 2
                && serde_json::from_str::<Value>(&parts[1]).is_ok_and(|options| {
                    RoomRandomRequest::parse(
                        "room.random.choose",
                        &json!({"options": options, "reason": ""}),
                    )
                    .is_ok()
                })
        }
        _ => false,
    }
}

#[cfg(unix)]
fn unsafe_shell_arguments(command: &str) -> bool {
    posix_shell_expansion_or_control(command)
}

#[cfg(windows)]
fn unsafe_shell_arguments(command: &str) -> bool {
    windows_shell_metacharacter(command)
}

#[cfg(unix)]
fn posix_shell_expansion_or_control(command: &str) -> bool {
    let mut single = false;
    let mut double = false;
    let mut escaped = false;
    for character in command.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        if character == '\\' && !single {
            escaped = true;
            continue;
        }
        if character == '\'' && !double {
            single = !single;
            continue;
        }
        if character == '"' && !single {
            double = !double;
            continue;
        }
        let substitution_or_control = !single
            && matches!(
                character,
                '$' | '`' | ';' | '&' | '|' | '<' | '>' | '(' | ')'
            );
        let unquoted_word_expansion =
            !single && !double && matches!(character, '*' | '?' | '[' | ']' | '{' | '}' | '~');
        if substitution_or_control || unquoted_word_expansion {
            return true;
        }
    }
    single || double || escaped
}

#[cfg(any(windows, test))]
fn windows_shell_metacharacter(command: &str) -> bool {
    let mut quoted = false;
    for character in command.chars() {
        if character.is_control() || matches!(character, '%' | '!' | '^' | '\'') {
            return true;
        }
        if character == '"' {
            quoted = !quoted;
            continue;
        }
        if !quoted && matches!(character, '&' | '|' | '<' | '>' | '(' | ')') {
            return true;
        }
    }
    quoted
}

fn valid_agent_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'))
}

#[cfg(test)]
mod tests {
    use super::{helper_tool, safe_room_command, windows_shell_metacharacter};

    #[cfg(unix)]
    use super::posix_shell_expansion_or_control;

    #[cfg(unix)]
    const HELPER: &str = "'/private/helper path/agentsassemble-room'";
    #[cfg(windows)]
    const HELPER: &str = r#""C:\private helper\agentsassemble-room.exe""#;
    const ATTACHMENT_ID: &str = "ma_11111111111111111111111111111111";

    #[test]
    fn history_commands_map_to_the_existing_roomportal_tools() {
        let (tool, payload) = helper_tool(
            "search",
            [
                "  old\r\ndeployment  ".to_owned(),
                "WzEyMyw0NTZd".to_owned(),
            ]
            .into_iter(),
        )
        .unwrap_or_else(|error| panic!("parse search helper command: {error}"));
        assert_eq!(tool, "search_messages");
        assert_eq!(payload["query"], "old  deployment");
        assert_eq!(payload["cursor"], "WzEyMyw0NTZd");

        let (tool, payload) = helper_tool("context", ["event-1".to_owned()].into_iter())
            .unwrap_or_else(|error| panic!("parse context helper command: {error}"));
        assert_eq!(tool, "read_message_context");
        assert_eq!(payload["event_id"], "event-1");
    }

    #[test]
    fn vote_commands_map_to_the_common_roomportal_tools() {
        let definition =
            r#"{"question":"Ship the helper?","options":["Yes","No"],"duration_seconds":300}"#;
        let (tool, payload) = helper_tool("vote-create", [definition.to_owned()].into_iter())
            .unwrap_or_else(|error| panic!("parse vote-create helper command: {error}"));
        assert_eq!(tool, "create_vote");
        assert_eq!(
            payload,
            serde_json::json!({
                "question": "Ship the helper?",
                "options": ["Yes", "No"],
                "duration_seconds": 300,
            })
        );

        for (action, arguments, expected_tool, expected_payload) in [
            (
                "vote-cast",
                vec!["poll-1".to_owned(), "Yes".to_owned()],
                "cast_vote",
                serde_json::json!({"vote_id": "poll-1", "choice": "Yes"}),
            ),
            (
                "vote-withdraw",
                vec!["poll-1".to_owned()],
                "withdraw_vote",
                serde_json::json!({"vote_id": "poll-1"}),
            ),
            (
                "vote-close",
                vec!["poll-1".to_owned()],
                "close_vote",
                serde_json::json!({"vote_id": "poll-1"}),
            ),
        ] {
            let (tool, payload) = helper_tool(action, arguments.into_iter())
                .unwrap_or_else(|error| panic!("parse {action} helper command: {error}"));
            assert_eq!(tool, expected_tool);
            assert_eq!(payload, expected_payload);
        }

        for (action, arguments) in [
            (
                "vote-create",
                vec![r#"{"question":"No","options":["A","B"],"extra":true}"#.to_owned()],
            ),
            (
                "vote-create",
                vec![r#"{"question":"No","options":["A","B"],"duration_seconds":29}"#.to_owned()],
            ),
            (
                "vote-cast",
                vec!["poll-1".to_owned(), "Yes".to_owned(), "extra".to_owned()],
            ),
            ("vote-close", vec!["poll-1".to_owned(), "extra".to_owned()]),
        ] {
            assert!(
                helper_tool(action, arguments.into_iter()).is_err(),
                "invalid {action} helper command was accepted"
            );
        }
    }

    #[test]
    fn hook_allows_only_one_exact_room_helper_command() {
        let message = if cfg!(windows) {
            "\"hello room\""
        } else {
            "'hello room'"
        };
        let targeted_message = if cfg!(windows) {
            "\"your turn\""
        } else {
            "'your turn'"
        };
        let roll = if cfg!(windows) {
            "\"2d6+1\""
        } else {
            "'2d6+1'"
        };
        let choices = if cfg!(windows) {
            r#"[\"north\",\"south\"]"#
        } else {
            r#"'["north","south"]'"#
        };
        let search_query = if cfg!(windows) {
            "\"old deployment\""
        } else {
            "'old deployment'"
        };
        let vote_definition = if cfg!(windows) {
            r#"{\"question\":\"Ship?\",\"options\":[\"Yes\",\"No\"]}"#
        } else {
            r#"'{"question":"Ship?","options":["Yes","No"]}'"#
        };
        let literal_pattern = if cfg!(windows) { "\"*\"" } else { "'*'" };
        for command in [
            format!("{HELPER} help"),
            format!("{HELPER} read"),
            format!("{HELPER} media {ATTACHMENT_ID}"),
            format!("{HELPER} search {search_query}"),
            format!("{HELPER} search {search_query} WzEyMyw0NTZd"),
            format!("{HELPER} context event-1"),
            format!("{HELPER} speak {literal_pattern}"),
            format!("{HELPER} speak {message}"),
            format!("{HELPER} speak-to agent-2 {targeted_message}"),
            format!("{HELPER} decline duplicate"),
            format!("{HELPER} vote-create {vote_definition}"),
            format!("{HELPER} vote-cast poll-1 Yes"),
            format!("{HELPER} vote-withdraw poll-1"),
            format!("{HELPER} vote-close poll-1"),
            format!("{HELPER} roll {roll}"),
            format!("{HELPER} choose {choices}"),
        ] {
            assert!(
                safe_room_command(&command, HELPER),
                "safe command rejected: {command}"
            );
        }
        for command in [
            format!("{HELPER} read && env"),
            format!("{HELPER} media ma_1111111111111111111111111111111Z"),
            format!("{HELPER} media {ATTACHMENT_ID} extra"),
            format!("{HELPER} search"),
            format!("{HELPER} search {search_query} cursor extra"),
            format!("{HELPER} context {}", "x".repeat(129)),
            format!("{HELPER} context event-1 extra"),
            format!("{HELPER} vote-create invalid-json"),
            format!("{HELPER} vote-cast poll-1 Yes extra"),
            format!("{HELPER} vote-withdraw poll-1 extra"),
            format!("{HELPER} vote-close poll-1 extra"),
            if cfg!(windows) {
                format!("{HELPER} speak ^& whoami")
            } else {
                format!("{HELPER} speak *")
            },
            if cfg!(windows) {
                format!("{HELPER} speak !USERPROFILE!")
            } else {
                format!("{HELPER} speak ~")
            },
            if cfg!(windows) {
                format!("{HELPER} speak \"%USERPROFILE%\"")
            } else {
                format!("{HELPER} speak \"$HOME\"")
            },
            format!("{HELPER} read\nuname"),
            "agentsassemble-room read".to_owned(),
            "/tmp/agentsassemble-room read".to_owned(),
        ] {
            assert!(
                !safe_room_command(&command, HELPER),
                "unsafe command allowed: {command}"
            );
        }
    }

    #[test]
    fn windows_grammar_never_treats_posix_quotes_as_protection() {
        assert!(windows_shell_metacharacter("speak 'safe & whoami'"));
        assert!(windows_shell_metacharacter("speak \"%USERPROFILE%\""));
        assert!(windows_shell_metacharacter("read ^& whoami"));
        assert!(!windows_shell_metacharacter("speak \"safe & literal\""));
        assert!(!windows_shell_metacharacter(
            "choose [\\\"north\\\",\\\"south\\\"]"
        ));
    }

    #[cfg(unix)]
    #[test]
    fn unix_grammar_rejects_unquoted_word_expansion() {
        for command in [
            "speak *",
            "speak .??*",
            "speak {README,AGENTS}.md",
            "speak ~",
        ] {
            assert!(posix_shell_expansion_or_control(command));
        }
        for command in ["speak '*'", "speak \"?\"", "speak \\*", "speak '~'"] {
            assert!(!posix_shell_expansion_or_control(command));
        }
    }
}
