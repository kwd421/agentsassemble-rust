pub(super) fn card_lines(mut remainder: &str) -> impl Iterator<Item = &str> {
    std::iter::from_fn(move || {
        if remainder.is_empty() {
            return None;
        }
        for (index, character) in remainder.char_indices() {
            if !is_card_line_boundary(character) {
                continue;
            }
            let mut separator_length = character.len_utf8();
            if character == '\r' && remainder[index + separator_length..].starts_with('\n') {
                separator_length += 1;
            }
            let line = &remainder[..index];
            remainder = &remainder[index + separator_length..];
            return Some(line);
        }
        let line = remainder;
        remainder = "";
        Some(line)
    })
}

pub fn trim_persona_card_text(value: &str) -> &str {
    value.trim_matches(is_persona_card_whitespace)
}

pub(super) fn prompt_card_text(value: &str, limit: usize) -> String {
    let mut output = String::new();
    let mut used = 0;
    for word in value
        .split(is_persona_card_whitespace)
        .filter(|word| !word.is_empty())
    {
        if used == limit {
            break;
        }
        if !output.is_empty() {
            output.push(' ');
            used += 1;
        }
        for character in word.chars() {
            if used == limit {
                return output;
            }
            output.push(character);
            used += 1;
        }
    }
    output
}

fn is_card_line_boundary(character: char) -> bool {
    matches!(
        character,
        '\n' | '\r'
            | '\u{000B}'
            | '\u{000C}'
            | '\u{001C}'
            | '\u{001D}'
            | '\u{001E}'
            | '\u{0085}'
            | '\u{2028}'
            | '\u{2029}'
    )
}

fn is_persona_card_whitespace(character: char) -> bool {
    character.is_whitespace() || matches!(character, '\u{001C}'..='\u{001F}')
}
