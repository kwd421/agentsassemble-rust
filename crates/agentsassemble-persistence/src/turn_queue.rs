use std::collections::HashSet;

use agentsassemble_domain::QueuedRoomInput;

pub(crate) const MAX_QUEUED_EVENT_IDS: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct InvalidTurnQueue;

pub(crate) fn merge_room_inputs<'a>(
    values: impl IntoIterator<Item = &'a QueuedRoomInput>,
) -> Result<Vec<QueuedRoomInput>, InvalidTurnQueue> {
    let mut seen = HashSet::with_capacity(MAX_QUEUED_EVENT_IDS);
    let mut merged = Vec::with_capacity(MAX_QUEUED_EVENT_IDS);
    for value in values {
        if merged.len() == MAX_QUEUED_EVENT_IDS
            || value.event_id.is_empty()
            || !seen.insert(value.event_id.as_str())
        {
            return Err(InvalidTurnQueue);
        }
        merged.push(value.clone());
    }
    Ok(merged)
}

pub(crate) fn room_input_queue_is_canonical<'a>(
    values: impl IntoIterator<Item = &'a QueuedRoomInput>,
) -> bool {
    merge_room_inputs(values).is_ok()
}

#[cfg(test)]
mod tests {
    use agentsassemble_domain::{QueuedRoomInput, RoomInputDeliveryKind};

    use super::{MAX_QUEUED_EVENT_IDS, merge_room_inputs, room_input_queue_is_canonical};

    #[test]
    fn queue_merge_rejects_noncanonical_or_oversized_authority() {
        let values = (0..MAX_QUEUED_EVENT_IDS + 32)
            .map(|index| QueuedRoomInput {
                event_id: format!("event-{index}"),
                delivery_kind: RoomInputDeliveryKind::OrderedObservation,
            })
            .collect::<Vec<_>>();
        let valid = values[..MAX_QUEUED_EVENT_IDS].to_vec();
        assert_eq!(merge_room_inputs(valid.iter()), Ok(valid.clone()));
        assert!(room_input_queue_is_canonical(valid.iter()));
        assert!(!room_input_queue_is_canonical(values.iter()));
        assert!(merge_room_inputs(values.iter()).is_err());
        assert!(merge_room_inputs(valid.iter().chain(valid.iter())).is_err());
    }
}
