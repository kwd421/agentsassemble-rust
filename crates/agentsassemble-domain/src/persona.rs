use std::collections::{BTreeMap, BTreeSet};

use caseless::default_case_fold_str;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use ts_rs::TS;
use unicode_general_category::{GeneralCategory, get_general_category};

use crate::persona_text::{card_lines, prompt_card_text, trim_persona_card_text};

pub const MAX_PERSONA_ID_CHARACTERS: usize = 80;
pub const MAX_PERSONA_CONTEXT_CHARACTERS: usize = 8_000;
pub const MAX_PERSONA_LORE_CHARACTERS: usize = 3_600;
const MAX_RECURSIVE_SCAN_DEPTH: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum PersonaAssetKind {
    Card,
    Module,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct PersonaAssetSummary {
    pub id: String,
    pub display_name: String,
    pub asset_kind: PersonaAssetKind,
    pub source_kind: String,
    pub lorebook_count: usize,
    pub asset_count: usize,
    pub ignored_feature_count: u64,
    pub tag_count: usize,
    pub thumbnail_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PersonaCard {
    pub id: String,
    pub display_name: String,
    pub description: String,
    pub system_prompt: String,
    pub personality: String,
    pub scenario: String,
    pub first_message: String,
    pub example_messages: String,
    pub post_history_instructions: String,
    pub lorebook: Vec<PersonaLoreEntry>,
    pub lore_settings: PersonaLoreSettings,
    pub asset_kind: PersonaAssetKind,
    pub source_kind: String,
    pub asset_count: usize,
    pub ignored_features: BTreeMap<String, u32>,
    pub tag_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools)] // Imported lore flags are independent card facts.
#[serde(deny_unknown_fields)]
pub struct PersonaLoreEntry {
    pub key: String,
    pub content: String,
    pub secondary_key: String,
    pub comment: String,
    pub always_active: bool,
    pub selective: bool,
    pub use_regex: bool,
    pub insert_order: i64,
    pub enabled: bool,
    pub case_sensitive: bool,
    pub priority: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PersonaLoreSettings {
    pub scan_depth: usize,
    pub recursive_scanning: bool,
    pub full_word_matching: bool,
}

impl Default for PersonaLoreSettings {
    fn default() -> Self {
        Self {
            scan_depth: 1,
            recursive_scanning: false,
            full_word_matching: false,
        }
    }
}

impl PersonaCard {
    #[must_use]
    pub fn summary(&self, thumbnail_available: bool) -> PersonaAssetSummary {
        PersonaAssetSummary {
            id: self.id.clone(),
            display_name: self.display_name.clone(),
            asset_kind: self.asset_kind,
            source_kind: self.source_kind.clone(),
            lorebook_count: self.lorebook.len(),
            asset_count: self.asset_count,
            ignored_feature_count: self
                .ignored_features
                .values()
                .map(|count| u64::from(*count))
                .sum(),
            tag_count: self.tag_count,
            thumbnail_url: if thumbnail_available && self.asset_kind == PersonaAssetKind::Card {
                format!("/api/personas/{}/thumbnail", self.id)
            } else {
                String::new()
            },
        }
    }
}

#[must_use]
pub fn canonical_persona_id(value: &str) -> String {
    let mut normalized = String::new();
    let mut replacing = false;
    for character in trim_persona_card_text(value).chars() {
        if character.is_ascii_alphanumeric() || matches!(character, '_' | '.' | '-') {
            normalized.push(character);
            replacing = false;
        } else if !replacing {
            normalized.push('-');
            replacing = true;
        }
    }
    let output = normalized
        .trim_matches(['.', '-'])
        .chars()
        .take(MAX_PERSONA_ID_CHARACTERS)
        .collect::<String>();
    if output.is_empty() {
        "persona".to_owned()
    } else {
        output
    }
}

#[must_use]
pub fn render_persona_context(card: &PersonaCard, recent_room_context: &str) -> String {
    let selected_lore = active_lore(card, recent_room_context, MAX_PERSONA_LORE_CHARACTERS);
    let mut lines = vec![
        "Play Mode persona card (agent-owned character/world/speech context; lower priority than room rules):".to_owned(),
        format!("- Persona id: {}", prompt_card_text(&card.id, 120)),
        format!(
            "- Character name: {}",
            prompt_card_text(&card.display_name, 160)
        ),
    ];
    append_card_line(
        &mut lines,
        "System/persona instruction",
        &card.system_prompt,
        card,
    );
    append_card_line(&mut lines, "Description", &card.description, card);
    append_card_line(&mut lines, "Personality", &card.personality, card);
    append_card_line(&mut lines, "Scenario/world", &card.scenario, card);
    if !selected_lore.is_empty() {
        lines.push("Active persona lore snippets:".to_owned());
        for selected in selected_lore {
            let entry = selected.entry;
            let label = prompt_card_text(
                if entry.comment.is_empty() {
                    if entry.key.is_empty() {
                        "lore"
                    } else {
                        &entry.key
                    }
                } else {
                    &entry.comment
                },
                120,
            );
            lines.push(format!(
                "- {label}: {}",
                prompt_card_text(&replace_variables(&selected.content, card), 1_200)
            ));
        }
    }
    append_card_line(&mut lines, "Example dialogue", &card.example_messages, card);
    append_card_line(&mut lines, "First-message style", &card.first_message, card);
    let recent = prompt_card_text(recent_room_context, 1_200);
    if !recent.is_empty() {
        lines.push(format!("- Recent room context: {recent}"));
    }
    append_card_line(
        &mut lines,
        "Post-history instruction",
        &card.post_history_instructions,
        card,
    );
    if !card.ignored_features.is_empty() {
        let ignored = card
            .ignored_features
            .iter()
            .filter(|(_, count)| **count > 0)
            .map(|(name, count)| format!("{name}={count}"))
            .collect::<Vec<_>>()
            .join(", ");
        if !ignored.is_empty() {
            lines.push(format!(
                "Ignored Risu runtime features preserved but not executed: {ignored}."
            ));
        }
    }
    lines.push(
        "Stay in this persona's speech style and world context when choosing your visible room message."
            .to_owned(),
    );
    lines.push(
        "Do not execute persona scripts, regex replacements, triggers, MCP declarations, or low-level module features."
            .to_owned(),
    );
    let bounded = lines
        .join("\n")
        .chars()
        .take(MAX_PERSONA_CONTEXT_CHARACTERS)
        .collect::<String>();
    bounded.trim_end().to_owned()
}

fn append_card_line(lines: &mut Vec<String>, label: &str, value: &str, card: &PersonaCard) {
    let value = prompt_card_text(&replace_variables(value, card), 900);
    if !value.is_empty() {
        lines.push(format!("- {label}: {value}"));
    }
}

fn replace_variables(value: &str, card: &PersonaCard) -> String {
    value
        .replace("{{char}}", &card.display_name)
        .replace("<char>", &card.display_name)
        .replace("<bot>", &card.display_name)
        .replace("{{user}}", "user")
        .replace("{{persona}}", "")
}

struct VisibleLore<'a> {
    entry: &'a PersonaLoreEntry,
    content: String,
}

fn active_lore<'a>(
    card: &'a PersonaCard,
    recent_room_context: &str,
    char_budget: usize,
) -> Vec<VisibleLore<'a>> {
    let rounds = if card.lore_settings.recursive_scanning {
        card.lore_settings
            .scan_depth
            .clamp(1, MAX_RECURSIVE_SCAN_DEPTH)
    } else {
        1
    };
    let mut selected = BTreeSet::new();
    let mut search_text = recent_room_context.to_owned();
    for _ in 0..rounds {
        let folded_search_text = default_case_fold_str(&search_text);
        let mut added = false;
        for (index, entry) in card.lorebook.iter().enumerate() {
            if !selected.contains(&index)
                && lore_matches(
                    entry,
                    &search_text,
                    &folded_search_text,
                    !recent_room_context.is_empty(),
                    card.lore_settings.full_word_matching,
                    index,
                )
            {
                selected.insert(index);
                added = true;
            }
        }
        if !added {
            break;
        }
        recent_room_context.clone_into(&mut search_text);
        for index in &selected {
            search_text.push('\n');
            search_text.push_str(&visible_lore_content(&card.lorebook[*index]));
        }
    }
    let mut ordered = selected.into_iter().collect::<Vec<_>>();
    ordered.sort_by_key(|index| (card.lorebook[*index].insert_order, *index));
    let visible = ordered
        .into_iter()
        .map(|index| VisibleLore {
            entry: &card.lorebook[index],
            content: visible_lore_content(&card.lorebook[index]),
        })
        .collect();
    fit_lore_budget(visible, char_budget)
}

fn fit_lore_budget(entries: Vec<VisibleLore<'_>>, char_budget: usize) -> Vec<VisibleLore<'_>> {
    if char_budget == 0 {
        return Vec::new();
    }
    let mut by_priority = (0..entries.len()).collect::<Vec<_>>();
    by_priority.sort_by_key(|position| {
        (
            std::cmp::Reverse(entries[*position].entry.priority),
            entries[*position].entry.insert_order,
            *position,
        )
    });
    let mut accepted_positions = BTreeSet::new();
    let mut used = 0_usize;
    let mut fallback = None;
    for position in by_priority {
        let length = entries[position].content.chars().count();
        if used.saturating_add(length) <= char_budget {
            accepted_positions.insert(position);
            used += length;
        } else if fallback.is_none() {
            fallback = Some(position);
        }
    }
    if accepted_positions.is_empty() {
        let Some(fallback) = fallback else {
            return Vec::new();
        };
        return entries
            .into_iter()
            .enumerate()
            .filter_map(|(position, mut entry)| {
                if position != fallback {
                    return None;
                }
                entry.content = entry.content.chars().take(char_budget).collect();
                Some(entry)
            })
            .collect();
    }
    entries
        .into_iter()
        .enumerate()
        .filter(|(position, _)| accepted_positions.contains(position))
        .map(|(_, entry)| entry)
        .collect()
}

fn lore_matches(
    entry: &PersonaLoreEntry,
    context: &str,
    folded_context: &str,
    has_recent_message: bool,
    default_full_word: bool,
    index: usize,
) -> bool {
    if !entry.enabled || entry.content.is_empty() || entry.use_regex {
        return false;
    }
    let decorators = LoreDecorators::parse(&entry.content);
    if decorators
        .activate_only_after
        .is_some_and(|after| usize::from(has_recent_message) < after)
        || decorators
            .activate_only_every
            .is_some_and(|every| every == 0 || usize::from(has_recent_message) % every != 0)
    {
        return false;
    }
    if let Some(probability) = decorators.probability {
        if probability <= 0 {
            return false;
        }
        if probability < 100 {
            let seed = format!("{}\n{}\n{index}", entry.key, entry.content);
            let digest = Sha256::digest(seed.as_bytes());
            let draw = u32::from_be_bytes([digest[0], digest[1], digest[2], digest[3]]) % 100;
            if i64::from(draw) >= probability {
                return false;
            }
        }
    }
    if entry.always_active {
        return true;
    }
    let full_word = if decorators.match_full_word {
        true
    } else if decorators.match_partial_word {
        false
    } else {
        default_full_word
    };
    let primary = keywords(&entry.key);
    let secondary = keywords(&entry.secondary_key);
    if primary.is_empty() && secondary.is_empty() {
        return false;
    }
    let primary_match = primary.iter().any(|keyword| {
        literal_match(
            context,
            folded_context,
            keyword,
            entry.case_sensitive,
            full_word,
        )
    });
    let secondary_match = secondary.iter().any(|keyword| {
        literal_match(
            context,
            folded_context,
            keyword,
            entry.case_sensitive,
            full_word,
        )
    });
    if entry.selective && !secondary.is_empty() {
        primary_match && secondary_match
    } else {
        primary_match || secondary_match
    }
}

fn literal_match(
    context: &str,
    folded_context: &str,
    keyword: &str,
    case_sensitive: bool,
    full_word: bool,
) -> bool {
    if keyword.is_empty() {
        return false;
    }
    let folded_keyword;
    let (haystack, needle) = if case_sensitive {
        (context, keyword)
    } else {
        folded_keyword = default_case_fold_str(keyword);
        (folded_context, folded_keyword.as_str())
    };
    if !full_word {
        return haystack.contains(needle);
    }
    if !case_sensitive {
        return haystack.char_indices().any(|(start, _)| {
            let Some(matched_len) = python_ignorecase_prefix_len(&haystack[start..], needle) else {
                return false;
            };
            has_python_word_boundaries(haystack, start, start + matched_len)
        });
    }
    let mut search_from = 0;
    while let Some(relative_start) = haystack[search_from..].find(needle) {
        let start = search_from + relative_start;
        let end = start + needle.len();
        if has_python_word_boundaries(haystack, start, end) {
            return true;
        }
        search_from = start
            + haystack[start..]
                .chars()
                .next()
                .map_or(needle.len(), char::len_utf8);
    }
    false
}

fn python_ignorecase_prefix_len(haystack: &str, needle: &str) -> Option<usize> {
    let mut consumed = 0;
    let mut haystack_chars = haystack.chars();
    for expected in needle.chars() {
        let actual = haystack_chars.next()?;
        if actual != expected && !matches!((actual, expected), ('i', '\u{131}') | ('\u{131}', 'i'))
        {
            return None;
        }
        consumed += actual.len_utf8();
    }
    Some(consumed)
}

fn has_python_word_boundaries(haystack: &str, start: usize, end: usize) -> bool {
    haystack[..start]
        .chars()
        .next_back()
        .is_none_or(|value| !is_python_word_character(value))
        && haystack[end..]
            .chars()
            .next()
            .is_none_or(|value| !is_python_word_character(value))
}

fn is_python_word_character(value: char) -> bool {
    value == '_'
        || matches!(
            get_general_category(value),
            GeneralCategory::UppercaseLetter
                | GeneralCategory::LowercaseLetter
                | GeneralCategory::TitlecaseLetter
                | GeneralCategory::ModifierLetter
                | GeneralCategory::OtherLetter
                | GeneralCategory::DecimalNumber
                | GeneralCategory::LetterNumber
                | GeneralCategory::OtherNumber
        )
}

fn keywords(value: &str) -> Vec<&str> {
    value
        .split([',', ';', '\n'])
        .map(trim_persona_card_text)
        .filter(|value| !value.is_empty())
        .collect()
}

fn visible_lore_content(entry: &PersonaLoreEntry) -> String {
    let mut content = String::new();
    let mut first = true;
    for line in
        card_lines(&entry.content).skip_while(|line| trim_persona_card_text(line).starts_with("@@"))
    {
        if !first {
            content.push('\n');
        }
        first = false;
        content.push_str(line);
    }
    content
}

#[derive(Default)]
struct LoreDecorators {
    activate_only_after: Option<usize>,
    activate_only_every: Option<usize>,
    match_full_word: bool,
    match_partial_word: bool,
    probability: Option<i64>,
}

impl LoreDecorators {
    fn parse(content: &str) -> Self {
        let mut result = Self::default();
        for line in card_lines(content) {
            let line = trim_persona_card_text(line);
            let Some(body) = line.strip_prefix("@@") else {
                break;
            };
            let mut parts = body.split_whitespace();
            match parts.next().unwrap_or_default() {
                "activate_only_after" => {
                    result.activate_only_after = parts.next().and_then(|value| value.parse().ok());
                }
                "activate_only_every" => {
                    result.activate_only_every = parts.next().and_then(|value| value.parse().ok());
                }
                "match_full_word" => result.match_full_word = true,
                "match_partial_word" => result.match_partial_word = true,
                "probability" => {
                    result.probability = parts.next().and_then(|value| value.parse().ok());
                }
                _ => {}
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{
        MAX_PERSONA_LORE_CHARACTERS, PersonaAssetKind, PersonaCard, PersonaLoreEntry,
        PersonaLoreSettings, canonical_persona_id, render_persona_context,
    };

    #[test]
    fn canonical_id_matches_reachable_ascii_contract() {
        assert_eq!(canonical_persona_id("  Mina / Guide  "), "Mina-Guide");
        assert_eq!(canonical_persona_id("..."), "persona");
    }

    #[test]
    fn summary_contains_no_private_prompt_body() {
        let card = card(vec![]);
        let summary = serde_json::to_value(card.summary(true))
            .unwrap_or_else(|error| panic!("serialize summary: {error}"));
        assert_eq!(summary["thumbnail_url"], "/api/personas/Mina/thumbnail");
        assert!(!summary.to_string().contains("private system"));
    }

    #[test]
    fn ordinary_context_applies_literal_lore_and_keeps_regex_inert() {
        let mut ignored = BTreeMap::new();
        ignored.insert("regex".to_owned(), 1);
        let mut card = card(vec![
            lore("harbor", "The harbor bell rings."),
            PersonaLoreEntry {
                use_regex: true,
                key: ".*".to_owned(),
                content: "EXECUTE_REGEX".to_owned(),
                ..lore("", "")
            },
        ]);
        card.ignored_features = ignored;
        let context = render_persona_context(&card, "We reached the harbor.");
        assert!(context.contains("The harbor bell rings."));
        assert!(!context.contains("EXECUTE_REGEX"));
        assert!(context.contains("Ignored Risu runtime features preserved but not executed"));
        assert!(context.contains("Character name: Mina"));
        assert!(context.contains("Hello Mina"));
    }

    #[test]
    fn recursive_literal_scan_uses_selected_lore_as_context() {
        let mut card = card(vec![
            lore("harbor", "A silver key is visible."),
            lore("silver key", "The vault opens."),
        ]);
        card.lore_settings.recursive_scanning = true;
        card.lore_settings.scan_depth = 2;
        let context = render_persona_context(&card, "harbor");
        assert!(context.contains("A silver key is visible."));
        assert!(context.contains("The vault opens."));
    }

    #[test]
    fn case_insensitive_lore_uses_full_unicode_case_folding() {
        let mut partial = lore("STRASSE", "The partial match activates.");
        partial.insert_order = 1;
        let mut full_word = lore(
            "STRASSE",
            "@@match_full_word\nThe full-word match activates.",
        );
        full_word.insert_order = 2;
        let context = render_persona_context(&card(vec![partial, full_word]), "Die Straße endet.");
        assert!(context.contains("The partial match activates."));
        assert!(context.contains("The full-word match activates."));
    }

    #[test]
    fn full_word_lore_uses_the_original_python_word_set() {
        let entry = lore(
            "cafe",
            "@@match_full_word\nThe combining-mark match activates.",
        );
        let context = render_persona_context(&card(vec![entry]), "cafe\u{301}");
        assert!(context.contains("The combining-mark match activates."));
    }

    #[test]
    fn full_word_lore_checks_overlapping_literal_occurrences() {
        let entry = lore("..", "@@match_full_word\nThe overlapping match activates.");
        let context = render_persona_context(&card(vec![entry]), "a...");
        assert!(context.contains("The overlapping match activates."));
    }

    #[test]
    fn full_word_lore_preserves_python_ignorecase_after_casefold() {
        let partial = lore("i", "The partial dotless-i match activates.");
        let full_word = lore(
            "i",
            "@@match_full_word\nThe full-word dotless-i match activates.",
        );
        let context = render_persona_context(&card(vec![partial, full_word]), "\u{131}");
        assert!(!context.contains("The partial dotless-i match activates."));
        assert!(context.contains("The full-word dotless-i match activates."));
    }

    #[test]
    fn lore_budget_counts_only_the_visible_content() {
        let mut high_priority = lore(
            "harbor",
            &format!(
                "@@match_full_word{}\nHIGH_PRIORITY_VISIBLE",
                " ".repeat(MAX_PERSONA_LORE_CHARACTERS)
            ),
        );
        high_priority.priority = 10;
        let low_priority = lore("harbor", "LOW_PRIORITY_VISIBLE");

        let context = render_persona_context(
            &card(vec![high_priority, low_priority]),
            "The harbor opens.",
        );
        assert!(context.contains("HIGH_PRIORITY_VISIBLE"));
        assert!(context.contains("LOW_PRIORITY_VISIBLE"));
    }

    #[test]
    fn oversized_lore_fallback_truncates_before_variable_replacement() {
        let lore_body = format!("{}OUTSIDE_BUDGET", "{{persona}}".repeat(400));
        let rendered = render_persona_context(&card(vec![lore("harbor", &lore_body)]), "harbor");
        assert!(!rendered.contains("OUTSIDE_BUDGET"));
    }

    #[test]
    fn lore_decorators_use_python_line_boundaries() {
        let separators = [
            "\n", "\r", "\r\n", "\u{000B}", "\u{000C}", "\u{001C}", "\u{001D}", "\u{001E}",
            "\u{0085}", "\u{2028}", "\u{2029}",
        ];
        for separator in separators {
            let mut persona = card(vec![lore(
                "har",
                &format!("@@match_partial_word{separator}VISIBLE"),
            )]);
            persona.lore_settings.full_word_matching = true;
            let context = render_persona_context(&persona, "harbor");
            assert!(
                context.contains("VISIBLE"),
                "separator {separator:?} did not split the decorator from its visible body"
            );
        }
        assert_eq!(
            super::visible_lore_content(&lore("", "@@match_full_word\n\nVISIBLE")),
            "\nVISIBLE"
        );
    }

    #[test]
    fn persona_text_uses_the_source_whitespace_set() {
        let mut persona = card(vec![lore(
            "\u{001F}har\u{001F}",
            "\u{001F}@@match_partial_word\u{001F}\nVISIBLE\u{001F}TEXT",
        )]);
        persona.lore_settings.full_word_matching = true;
        let context = render_persona_context(&persona, "harbor");
        assert!(context.contains("VISIBLE TEXT"));
        assert!(!context.contains('\u{001F}'));
    }

    fn card(lorebook: Vec<PersonaLoreEntry>) -> PersonaCard {
        PersonaCard {
            id: "Mina".to_owned(),
            display_name: "Mina".to_owned(),
            description: "private description".to_owned(),
            system_prompt: "private system".to_owned(),
            personality: "steady".to_owned(),
            scenario: "port".to_owned(),
            first_message: "Hello {{char}}".to_owned(),
            example_messages: String::new(),
            post_history_instructions: String::new(),
            lorebook,
            lore_settings: PersonaLoreSettings::default(),
            asset_kind: PersonaAssetKind::Card,
            source_kind: "ccv3".to_owned(),
            asset_count: 1,
            ignored_features: BTreeMap::new(),
            tag_count: 2,
        }
    }

    fn lore(key: &str, content: &str) -> PersonaLoreEntry {
        PersonaLoreEntry {
            key: key.to_owned(),
            content: content.to_owned(),
            secondary_key: String::new(),
            comment: String::new(),
            always_active: false,
            selective: false,
            use_regex: false,
            insert_order: 0,
            enabled: true,
            case_sensitive: false,
            priority: 0,
        }
    }
}
