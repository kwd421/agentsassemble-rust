use agentsassemble_domain::{RoomRandomRequest, is_message_attachment_id};
use serde_json::{Value, json};

pub(super) fn helper_tool(
    action: &str,
    mut arguments: impl Iterator<Item = String>,
) -> Result<(&'static str, Value), &'static str> {
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
        "decline" => {
            parts.len() == 2
                && matches!(
                    parts[1].as_str(),
                    "nothing_useful_to_add" | "not_addressed" | "duplicate"
                )
        }
        "speak" => parts.len() >= 2 && parts[1..].iter().all(|part| !part.starts_with('~')),
        "speak-to" => {
            parts.len() >= 3
                && valid_agent_id(&parts[1])
                && parts[2..].iter().all(|part| !part.starts_with('~'))
        }
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
    shell_metacharacter_outside_single_quotes(command)
}

#[cfg(windows)]
fn unsafe_shell_arguments(command: &str) -> bool {
    windows_shell_metacharacter(command)
}

#[cfg(unix)]
fn shell_metacharacter_outside_single_quotes(command: &str) -> bool {
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
        if !single
            && matches!(
                character,
                '$' | '`' | ';' | '&' | '|' | '<' | '>' | '(' | ')'
            )
        {
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
    use super::windows_shell_metacharacter;

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
}
