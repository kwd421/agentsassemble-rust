use std::collections::HashSet;

pub(crate) const MAX_QUEUED_EVENT_IDS: usize = 256;

pub(crate) fn bounded_event_ids<'a>(values: impl IntoIterator<Item = &'a String>) -> Vec<String> {
    let mut seen = HashSet::with_capacity(MAX_QUEUED_EVENT_IDS);
    let mut bounded = Vec::with_capacity(MAX_QUEUED_EVENT_IDS);
    for value in values {
        if bounded.len() == MAX_QUEUED_EVENT_IDS {
            break;
        }
        if !value.is_empty() && seen.insert(value.as_str()) {
            bounded.push(value.clone());
        }
    }
    bounded
}

pub(crate) fn event_id_queue_is_canonical<'a>(
    values: impl IntoIterator<Item = &'a String>,
) -> bool {
    let mut seen = HashSet::with_capacity(MAX_QUEUED_EVENT_IDS);
    let mut count = 0_usize;
    for value in values {
        count = count.saturating_add(1);
        if count > MAX_QUEUED_EVENT_IDS || value.is_empty() || !seen.insert(value.as_str()) {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::{MAX_QUEUED_EVENT_IDS, bounded_event_ids, event_id_queue_is_canonical};

    #[test]
    fn queue_merge_is_deduplicated_and_hard_bounded() {
        let values = (0..MAX_QUEUED_EVENT_IDS + 32)
            .map(|index| format!("event-{index}"))
            .collect::<Vec<_>>();
        let duplicated = values.iter().chain(values.iter());
        let bounded = bounded_event_ids(duplicated);
        assert_eq!(bounded.len(), MAX_QUEUED_EVENT_IDS);
        assert_eq!(bounded.first().map(String::as_str), Some("event-0"));
        assert_eq!(bounded.last().map(String::as_str), Some("event-255"));
        assert!(event_id_queue_is_canonical(bounded.iter()));
        assert!(!event_id_queue_is_canonical(values.iter()));
    }
}
