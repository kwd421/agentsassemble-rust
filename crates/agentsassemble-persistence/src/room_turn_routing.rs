use std::collections::BTreeMap;

use agentsassemble_domain::DurableAgentSession;
use uuid::Uuid;

pub(super) fn last_direct_target<'a>(
    content: &str,
    sessions: impl Iterator<Item = &'a DurableAgentSession>,
) -> Option<String> {
    let mut aliases = BTreeMap::<String, Option<String>>::new();
    for session in sessions {
        let session_id = &session.public.session_id;
        for candidate in std::iter::once(session_id.as_str())
            .chain(std::iter::once(session.public.display_name.as_str()))
            .chain(session.public.display_name.split(is_alias_separator))
        {
            let alias = candidate
                .trim_matches(|character: char| " @,;!?[]{}".contains(character))
                .to_lowercase();
            if !valid_alias(&alias) || alias == "all" {
                continue;
            }
            aliases
                .entry(alias)
                .and_modify(|owner| {
                    if owner.as_deref() != Some(session_id) {
                        *owner = None;
                    }
                })
                .or_insert_with(|| Some(session_id.clone()));
        }
    }
    let content = content.to_lowercase();
    aliases
        .into_iter()
        .filter_map(|(alias, owner)| {
            let owner = owner?;
            last_mention_position(&content, &alias).map(|position| (position, owner))
        })
        .max()
        .map(|(_, owner)| owner)
}

pub(super) fn sampled_candidate_indexes(length: usize) -> Vec<usize> {
    if length <= 2 {
        return (0..length).collect();
    }
    let seed = Uuid::new_v4().as_u128();
    let divisor = u128::try_from(length).unwrap_or(u128::MAX);
    let first = usize::try_from(seed % divisor).unwrap_or(0);
    let mut second =
        usize::try_from((seed / divisor) % u128::try_from(length - 1).unwrap_or(u128::MAX))
            .unwrap_or(0);
    if second >= first {
        second += 1;
    }
    vec![first, second]
}

fn last_mention_position(content: &str, alias: &str) -> Option<usize> {
    let needle = format!("@{alias}");
    let plain = content
        .match_indices(&needle)
        .filter(|(index, _)| mention_boundaries(content, *index, needle.len()))
        .map(|(index, _)| index)
        .max();
    let bracketed = content
        .match_indices("<@")
        .filter_map(|(index, _)| bracketed_alias_matches(content, index, alias).then_some(index))
        .max();
    plain.max(bracketed)
}

fn bracketed_alias_matches(content: &str, index: usize, alias: &str) -> bool {
    let Some(rest) = content.get(index + 2..) else {
        return false;
    };
    let rest = rest.trim_start_matches(char::is_whitespace);
    let Some(rest) = rest.strip_prefix(alias) else {
        return false;
    };
    rest.trim_start_matches(char::is_whitespace)
        .starts_with('>')
}

fn mention_boundaries(content: &str, index: usize, length: usize) -> bool {
    let before = content[..index].chars().next_back();
    let after = content[index + length..].chars().next();
    before.is_none_or(|character| !mention_word(character))
        && after.is_none_or(|character| !mention_word(character))
}

fn mention_word(character: char) -> bool {
    character.is_alphanumeric() || matches!(character, '_' | '-')
}

fn valid_alias(alias: &str) -> bool {
    !alias.is_empty()
        && !alias
            .chars()
            .any(|character| matches!(character, '@' | '<' | '>' | '\r' | '\n'))
        && (alias.chars().any(char::is_whitespace)
            || alias.chars().all(|character| {
                character.is_alphanumeric() || matches!(character, '_' | '.' | '-')
            }))
}

fn is_alias_separator(character: char) -> bool {
    character.is_whitespace() || matches!(character, '/' | '|' | '·' | '—' | '–' | ':' | '(' | ')')
}

#[cfg(test)]
mod tests {
    use agentsassemble_domain::DurableAgentSession;

    use super::last_direct_target;

    #[test]
    fn final_unique_alias_owns_the_ordered_floor() {
        let terra = session("codex-1", "Codex Terra");
        let flash = session("antigravity-1", "Antigravity Flash");
        let sessions = [&terra, &flash];
        assert_eq!(
            last_direct_target(
                "@Terra recap that, then <@ antigravity-1 > answer",
                sessions.into_iter()
            ),
            Some("antigravity-1".to_owned())
        );
    }

    #[test]
    fn ambiguous_split_alias_does_not_route() {
        let first = session("codex-1", "Codex Terra");
        let second = session("codex-2", "Other Terra");
        let sessions = [&first, &second];
        assert_eq!(
            last_direct_target("@Terra answer", sessions.into_iter()),
            None
        );
    }

    fn session(id: &str, display_name: &str) -> DurableAgentSession {
        let mut value = serde_json::json!({
            "room_id": "general",
            "session_id": id,
            "participant_id": id,
            "display_name": display_name,
            "status": "attached",
            "runtime_status": "idle",
            "enabled": true,
            "provider_kind": "test",
            "runtime_kind": "test",
            "connection_kind": "test",
            "external_owned": false,
            "process_ownership": "server",
            "model": "test",
            "reasoning_effort": "",
            "service_tier": "",
            "variant": "",
            "execution_harness": "builtin",
            "permission_mode": "meeting_read_only",
            "max_output_tokens": 0,
            "catalog_revision": "test",
            "transport": "test",
            "last_seen_event_id": "",
            "last_seen_seq": 0,
            "last_provider_sync_event_id": "",
            "last_provider_sync_seq": 0,
            "bootstrap_cutoff_seq": 0,
            "turn_count": 0,
            "created_at": "2026-08-23T00:00:00Z",
            "updated_at": "2026-08-23T00:00:00Z",
            "workspace": "/test",
            "runtime_profile_key": "test"
        });
        value["provider_session_active"] = serde_json::json!(true);
        serde_json::from_value(value)
            .unwrap_or_else(|error| panic!("decode routing fixture: {error}"))
    }
}
