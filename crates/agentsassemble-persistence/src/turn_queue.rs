use std::collections::HashSet;

pub(crate) const MAX_QUEUED_EVENT_IDS: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct InvalidTurnQueue;

pub(crate) fn merge_event_ids<'a>(
    values: impl IntoIterator<Item = &'a String>,
) -> Result<Vec<String>, InvalidTurnQueue> {
    let mut seen = HashSet::with_capacity(MAX_QUEUED_EVENT_IDS);
    let mut merged = Vec::with_capacity(MAX_QUEUED_EVENT_IDS);
    for value in values {
        if merged.len() == MAX_QUEUED_EVENT_IDS || value.is_empty() || !seen.insert(value.as_str())
        {
            return Err(InvalidTurnQueue);
        }
        merged.push(value.clone());
    }
    Ok(merged)
}

pub(crate) fn event_id_queue_is_canonical<'a>(
    values: impl IntoIterator<Item = &'a String>,
) -> bool {
    merge_event_ids(values).is_ok()
}

#[cfg(test)]
mod tests {
    use super::{MAX_QUEUED_EVENT_IDS, event_id_queue_is_canonical, merge_event_ids};

    #[test]
    fn queue_merge_rejects_noncanonical_or_oversized_authority() {
        let values = (0..MAX_QUEUED_EVENT_IDS + 32)
            .map(|index| format!("event-{index}"))
            .collect::<Vec<_>>();
        let valid = values[..MAX_QUEUED_EVENT_IDS].to_vec();
        assert_eq!(merge_event_ids(valid.iter()), Ok(valid.clone()));
        assert!(event_id_queue_is_canonical(valid.iter()));
        assert!(!event_id_queue_is_canonical(values.iter()));
        assert!(merge_event_ids(values.iter()).is_err());
        assert!(merge_event_ids(valid.iter().chain(valid.iter())).is_err());
    }
}
