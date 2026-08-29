use crate::room_portal_terminal::HookApproval;

const APPROVE_CONVERSATION: &[u8] = b"\x1b[B\r";
const APPROVE_FILE_ONCE: &[u8] = b"\r";
const SANDBOX_APPROVAL_PROMPT: &str = "Allow sandbox bypass for command execution?";
const FILE_APPROVAL_PROMPT: &str = "Allow access to this file?";
const MAX_SCREEN_ROWS: usize = 400;
const MAX_SCREEN_COLUMNS: usize = 400;

pub(crate) struct AntigravityRoomPermissionPolicy {
    visible_approval: Option<HookApproval>,
}

impl AntigravityRoomPermissionPolicy {
    pub(crate) const fn new() -> Self {
        Self {
            visible_approval: None,
        }
    }

    pub(crate) fn begin_turn(&mut self) {
        self.visible_approval = None;
    }

    pub(crate) fn request_pending(&mut self, output: &[u8]) -> Option<HookApproval> {
        let screen = render_terminal_screen(output);
        let approval = if screen.contains(SANDBOX_APPROVAL_PROMPT) {
            Some(HookApproval::RunCommand)
        } else if screen.contains("File access")
            && screen.contains("Read:")
            && screen.contains(FILE_APPROVAL_PROMPT)
        {
            Some(HookApproval::ViewFile)
        } else {
            None
        };
        if approval.is_none() {
            self.visible_approval = None;
        }
        (approval != self.visible_approval)
            .then_some(approval)
            .flatten()
    }

    pub(crate) fn approve(&mut self, approval: HookApproval) -> &'static [u8] {
        self.visible_approval = Some(approval);
        match approval {
            HookApproval::RunCommand => APPROVE_CONVERSATION,
            HookApproval::ViewFile => APPROVE_FILE_ONCE,
        }
    }
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
    use super::{APPROVE_CONVERSATION, APPROVE_FILE_ONCE, AntigravityRoomPermissionPolicy};
    use crate::room_portal_terminal::HookApproval;

    #[test]
    fn recognizes_the_current_sandbox_card_once_without_parsing_wrapped_commands() {
        let mut policy = AntigravityRoomPermissionPolicy::new();
        let prompt = b"Requesting permission for:\r\n /private/agentsassemble-room read\r\n\
            \xf0\x9f\x94\x93 Allow sandbox bypass for command execution?";
        assert_eq!(
            policy.request_pending(prompt),
            Some(HookApproval::RunCommand)
        );
        assert_eq!(
            policy.approve(HookApproval::RunCommand),
            APPROVE_CONVERSATION
        );
        assert_eq!(policy.request_pending(prompt), None);
    }

    #[test]
    fn ignores_generic_confirmation_text() {
        let mut policy = AntigravityRoomPermissionPolicy::new();
        assert_eq!(
            policy
                .request_pending(b"Requesting permission for: git status\nDo you want to proceed?"),
            None
        );
    }

    #[test]
    fn reconstructs_a_cursor_positioned_wrapped_permission_card() {
        let mut policy = AntigravityRoomPermissionPolicy::new();
        let prompt = b"\x1b[2J\x1b[1;1HRequesting permission for:\x1b[3;1H/private/very-long-helper-\x1b[4;1Hpath/agentsassemble-room\x1b[5;1Hhelp\x1b[7;1HAllow sandbox bypass for command execution?";
        assert_eq!(
            policy.request_pending(prompt),
            Some(HookApproval::RunCommand)
        );
    }

    #[test]
    fn approves_each_private_file_card_once_without_persistent_access() {
        let mut policy = AntigravityRoomPermissionPolicy::new();
        let prompt = b"File access\r\nRead: /private/room-media/id/proof.txt\r\nReason: outside workspace\r\nAllow access to this file?";
        assert_eq!(policy.request_pending(prompt), Some(HookApproval::ViewFile));
        assert_eq!(policy.approve(HookApproval::ViewFile), APPROVE_FILE_ONCE);
        assert_eq!(policy.request_pending(prompt), None);
        assert_eq!(policy.request_pending(b"Reading file..."), None);
        assert_eq!(policy.request_pending(prompt), Some(HookApproval::ViewFile));
    }
}
