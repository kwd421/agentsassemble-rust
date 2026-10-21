//! Which external AIs are working on a message they received. The server sees
//! only their Room Connector requests: a wait that hands over someone else's
//! message starts the work, and the next wait, a posted message or leaving
//! ends it. Nothing is stored, so a restart forgets it.
use std::{
    collections::{BTreeSet, HashMap},
    sync::{Arc, Mutex},
    time::Duration,
};

use tokio::sync::watch;

/// Work that never reports back stops showing after this long.
const MAX_ACTIVE: Duration = Duration::from_mins(15);

#[derive(Clone, Default)]
pub(crate) struct ParticipantActivity {
    rooms: Arc<Mutex<HashMap<String, RoomActivity>>>,
}

struct RoomActivity {
    active: watch::Sender<BTreeSet<String>>,
    started: HashMap<String, u64>,
    next: u64,
}

impl RoomActivity {
    fn new() -> Self {
        Self {
            active: watch::channel(BTreeSet::new()).0,
            started: HashMap::new(),
            next: 0,
        }
    }
}

impl ParticipantActivity {
    pub(crate) fn subscribe(&self, room_id: &str) -> watch::Receiver<BTreeSet<String>> {
        let mut rooms = self
            .rooms
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        rooms
            .entry(room_id.to_owned())
            .or_insert_with(RoomActivity::new)
            .active
            .subscribe()
    }

    pub(crate) fn start(&self, room_id: &str, participant_id: &str) {
        let started = {
            let mut rooms = self
                .rooms
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let room = rooms
                .entry(room_id.to_owned())
                .or_insert_with(RoomActivity::new);
            room.next += 1;
            room.started.insert(participant_id.to_owned(), room.next);
            room.active
                .send_if_modified(|active| active.insert(participant_id.to_owned()));
            room.next
        };
        let activity = self.clone();
        let (room_id, participant_id) = (room_id.to_owned(), participant_id.to_owned());
        tokio::spawn(async move {
            tokio::time::sleep(MAX_ACTIVE).await;
            activity.end_started(&room_id, &participant_id, Some(started));
        });
    }

    pub(crate) fn end(&self, room_id: &str, participant_id: &str) {
        self.end_started(room_id, participant_id, None);
    }

    /// `started` limits the end to that exact start, so an old timer never
    /// ends newer work.
    fn end_started(&self, room_id: &str, participant_id: &str, started: Option<u64>) {
        let mut rooms = self
            .rooms
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(room) = rooms.get_mut(room_id) else {
            return;
        };
        if started.is_some_and(|started| room.started.get(participant_id) != Some(&started)) {
            return;
        }
        room.started.remove(participant_id);
        room.active
            .send_if_modified(|active| active.remove(participant_id));
        if room.started.is_empty() && room.active.receiver_count() == 0 {
            rooms.remove(room_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ParticipantActivity;

    #[tokio::test(start_paused = true)]
    async fn work_starts_ends_and_times_out() {
        let activity = ParticipantActivity::default();
        let mut active = activity.subscribe("room");
        activity.start("room", "connector-a");
        assert!(active.borrow_and_update().contains("connector-a"));
        activity.end("room", "connector-a");
        assert!(active.borrow_and_update().is_empty());
        activity.start("room", "connector-a");
        tokio::time::advance(super::MAX_ACTIVE / 2).await;
        // A later start keeps the work alive past the first start's deadline.
        activity.start("room", "connector-a");
        tokio::time::advance(super::MAX_ACTIVE / 2 + std::time::Duration::from_secs(1)).await;
        tokio::task::yield_now().await;
        assert!(active.borrow_and_update().contains("connector-a"));
        tokio::time::advance(super::MAX_ACTIVE).await;
        tokio::task::yield_now().await;
        assert!(active.borrow_and_update().is_empty());
    }
}
