use std::{collections::HashSet, sync::OnceLock};

use regex::Regex;

use crate::room_portal_terminal::safe_room_command;

const APPROVE_CONVERSATION: &[u8] = b"\x1b[B\r";
const MAX_SCREEN_ROWS: usize = 400;
const MAX_SCREEN_COLUMNS: usize = 400;

#[derive(Default)]
pub(crate) struct AntigravityRoomPermissionPolicy {
    handled_commands: HashSet<String>,
}

impl AntigravityRoomPermissionPolicy {
    pub(crate) fn begin_turn(&mut self) {
        self.handled_commands.clear();
    }

    pub(crate) fn response_for(&mut self, output: &[u8]) -> Result<Option<&'static [u8]>, ()> {
        let command = [render_terminal_screen(output), strip_terminal_ansi(output)]
            .into_iter()
            .find_map(|candidate| latest_permission_command(&candidate));
        let Some(command) = command else {
            return Ok(None);
        };
        if self.handled_commands.contains(&command) {
            return Ok(None);
        }
        if !safe_room_command(&command) {
            return Err(());
        }
        self.handled_commands.insert(command);
        Ok(Some(APPROVE_CONVERSATION))
    }
}

fn latest_permission_command(text: &str) -> Option<String> {
    static PERMISSION: OnceLock<Regex> = OnceLock::new();
    let expression = PERMISSION.get_or_init(|| {
        Regex::new(
            r"(?is)Requesting permission for:\s*(?P<command>.+?)\s*(?:Do you want to proceed\?|(?:🔓\s*)?Allow sandbox bypass for command execution\?)",
        )
        .unwrap_or_else(|error| panic!("static Antigravity permission regex is invalid: {error}"))
    });
    let command = expression
        .captures_iter(text)
        .last()?
        .name("command")?
        .as_str()
        .trim();
    if command.contains(['\r', '\n']) {
        Some(command.to_owned())
    } else {
        Some(command.split_whitespace().collect::<Vec<_>>().join(" "))
    }
}

fn strip_terminal_ansi(raw: &[u8]) -> String {
    let text = String::from_utf8_lossy(raw);
    let mut output = String::with_capacity(text.len());
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        if character != '\u{1b}' {
            if character != '\u{7}' {
                output.push(if character == '\r' { '\n' } else { character });
            }
            continue;
        }
        match characters.next() {
            Some('[') => {
                for next in characters.by_ref() {
                    if ('@'..='~').contains(&next) {
                        break;
                    }
                }
            }
            Some(']') => {
                let mut escape = false;
                for next in characters.by_ref() {
                    if next == '\u{7}' || (escape && next == '\\') {
                        break;
                    }
                    escape = next == '\u{1b}';
                }
            }
            Some(_) | None => {}
        }
    }
    output
}

fn render_terminal_screen(raw: &[u8]) -> String {
    let text = String::from_utf8_lossy(raw);
    let characters = text.chars().collect::<Vec<_>>();
    let mut screen: Vec<Vec<char>> = Vec::new();
    let (mut row, mut column, mut index) = (0_usize, 0_usize, 0_usize);
    while index < characters.len() {
        let character = characters[index];
        if character == '\u{1b}' {
            if characters.get(index + 1) == Some(&'[') {
                let (next, parameters, final_character) = parse_csi(&characters, index + 2);
                apply_csi(
                    &mut screen,
                    &mut row,
                    &mut column,
                    &parameters,
                    final_character,
                );
                index = next;
            } else {
                index = index.saturating_add(2);
            }
            continue;
        }
        match character {
            '\n' => row = row.saturating_add(1),
            '\r' => column = 0,
            '\u{8}' => column = column.saturating_sub(1),
            '\t' => column = (column / 8 + 1) * 8,
            '\u{7}' => {}
            _ if row < MAX_SCREEN_ROWS && column < MAX_SCREEN_COLUMNS => {
                let line = screen_row(&mut screen, row);
                if line.len() <= column {
                    line.resize(column + 1, ' ');
                }
                line[column] = character;
                column += 1;
            }
            _ => {}
        }
        index += 1;
    }
    screen
        .into_iter()
        .map(|line| line.into_iter().collect::<String>().trim_end().to_owned())
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_owned()
}

fn parse_csi(characters: &[char], mut index: usize) -> (usize, Vec<usize>, char) {
    let start = index;
    while index < characters.len() && !('@'..='~').contains(&characters[index]) {
        index += 1;
    }
    let final_character = characters.get(index).copied().unwrap_or_default();
    let parameters = characters[start..index]
        .iter()
        .collect::<String>()
        .trim_start_matches('?')
        .split(';')
        .filter_map(|value| value.parse().ok())
        .collect();
    (index.saturating_add(1), parameters, final_character)
}

fn apply_csi(
    screen: &mut Vec<Vec<char>>,
    row: &mut usize,
    column: &mut usize,
    parameters: &[usize],
    final_character: char,
) {
    let first = parameters.first().copied().unwrap_or(0);
    match final_character {
        'H' | 'f' => {
            *row = parameters.first().copied().unwrap_or(1).saturating_sub(1);
            *column = parameters.get(1).copied().unwrap_or(1).saturating_sub(1);
        }
        'A' => *row = row.saturating_sub(first.max(1)),
        'B' => *row = row.saturating_add(first.max(1)),
        'C' => *column = column.saturating_add(first.max(1)),
        'D' => *column = column.saturating_sub(first.max(1)),
        'G' => *column = first.max(1) - 1,
        'K' => erase_line(screen_row(screen, *row), *column, first),
        'J' if first == 2 => {
            screen.clear();
            *row = 0;
            *column = 0;
        }
        'J' if first == 0 => {
            screen_row(screen, *row).truncate(*column);
            screen.truncate(row.saturating_add(1));
        }
        _ => {}
    }
}

fn screen_row(screen: &mut Vec<Vec<char>>, row: usize) -> &mut Vec<char> {
    if screen.len() <= row {
        screen.resize_with(row + 1, Vec::new);
    }
    &mut screen[row]
}

fn erase_line(line: &mut Vec<char>, column: usize, mode: usize) {
    match mode {
        0 => line.truncate(column),
        1 => {
            for character in line.iter_mut().take(column.saturating_add(1)) {
                *character = ' ';
            }
        }
        _ => line.clear(),
    }
}

#[cfg(test)]
mod tests {
    use super::{APPROVE_CONVERSATION, AntigravityRoomPermissionPolicy};

    #[test]
    fn approves_an_exact_room_command_once() {
        let mut policy = AntigravityRoomPermissionPolicy::default();
        let prompt = b"Requesting permission for:\r\n agentsassemble-room read\r\n\
            \xf0\x9f\x94\x93 Allow sandbox bypass for command execution?";
        assert_eq!(
            policy.response_for(prompt).unwrap_or_default(),
            Some(APPROVE_CONVERSATION)
        );
        assert_eq!(policy.response_for(prompt).unwrap_or_default(), None);
    }

    #[test]
    fn rejects_shell_chaining_and_hidden_continuation_lines() {
        let mut policy = AntigravityRoomPermissionPolicy::default();
        for prompt in [
            b"Requesting permission for: agentsassemble-room read && env\nDo you want to proceed?"
                .as_slice(),
            b"Requesting permission for:\n agentsassemble-room speak 'safe'\n whoami\nDo you want to proceed?"
                .as_slice(),
        ] {
            assert!(policy.response_for(prompt).is_err());
            policy.begin_turn();
        }
    }

    #[test]
    fn reconstructs_a_cursor_positioned_permission_card() {
        let mut policy = AntigravityRoomPermissionPolicy::default();
        let prompt = b"\x1b[2J\x1b[1;1HRequesting permission for:\x1b[2;4Hagentsassemble-room help\x1b[3;1HDo you want to proceed?";
        assert_eq!(
            policy.response_for(prompt).unwrap_or_default(),
            Some(APPROVE_CONVERSATION)
        );
    }
}
