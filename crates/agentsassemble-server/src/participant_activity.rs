//! Which external AIs are working on a message they received, and which are
//! listening. The server sees only their Room Connector requests: a wait that
//! hands over someone else's message starts the work, and the next wait, a
//! posted message or leaving ends it; a wait left pending is listening. Nothing
//! is stored, so a restart forgets it.
use std::{
    collections::{BTreeSet, HashMap},
    sync::{Arc, Mutex},
    time::Duration,
};

use tokio::sync::watch;

/// Work that never reports back stops showing after this long.
const MAX_ACTIVE: Duration = Duration::from_mins(15);
/// A wait ended and re-called at once (a hosted wait answers every 45 s) keeps
/// listening without a flicker.
const LISTEN_GRACE: Duration = Duration::from_secs(5);

/// Working and listening external AIs in one room.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ActivitySnapshot {
    pub(crate) working: BTreeSet<String>,
    pub(crate) listening: BTreeSet<String>,
}

#[derive(Clone, Default)]
pub(crate) struct ParticipantActivity {
    rooms: Arc<Mutex<HashMap<String, RoomActivity>>>,
}

struct RoomActivity {
    active: watch::Sender<ActivitySnapshot>,
    started: HashMap<String, u64>,
    /// Pending waits per participant, and the generation of its last wait end.
    waits: HashMap<String, (usize, u64)>,
    next: u64,
}

impl RoomActivity {
    fn new() -> Self {
        Self {
            active: watch::channel(ActivitySnapshot::default()).0,
            started: HashMap::new(),
            waits: HashMap::new(),
            next: 0,
        }
    }

    fn unused(&self) -> bool {
        self.started.is_empty() && self.waits.is_empty() && self.active.receiver_count() == 0
    }
}

/// Holds one pending wait; dropping it (answered, cancelled or disconnected)
/// stops listening after the grace unless another wait arrived meanwhile.
pub(crate) struct ListeningWait {
    activity: ParticipantActivity,
    room_id: String,
    participant_id: String,
}

impl Drop for ListeningWait {
    fn drop(&mut self) {
        let ended = {
            let mut rooms = self
                .activity
                .rooms
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let Some(room) = rooms.get_mut(&self.room_id) else {
                return;
            };
            room.next += 1;
            let generation = room.next;
            let Some(wait) = room.waits.get_mut(&self.participant_id) else {
                return;
            };
            wait.0 = wait.0.saturating_sub(1);
            wait.1 = generation;
            (wait.0 == 0).then_some(generation)
        };
        let Some(generation) = ended else {
            return;
        };
        let activity = self.activity.clone();
        let (room_id, participant_id) = (self.room_id.clone(), self.participant_id.clone());
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            activity.stop_listening(&room_id, &participant_id, generation);
            return;
        };
        runtime.spawn(async move {
            tokio::time::sleep(LISTEN_GRACE).await;
            activity.stop_listening(&room_id, &participant_id, generation);
        });
    }
}

impl ParticipantActivity {
    pub(crate) fn subscribe(&self, room_id: &str) -> watch::Receiver<ActivitySnapshot> {
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
                .send_if_modified(|active| active.working.insert(participant_id.to_owned()));
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
            .send_if_modified(|active| active.working.remove(participant_id));
        if room.unused() {
            rooms.remove(room_id);
        }
    }

    /// Marks a pending wait; the participant listens until the guard drops.
    pub(crate) fn listen(&self, room_id: &str, participant_id: &str) -> ListeningWait {
        let mut rooms = self
            .rooms
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let room = rooms
            .entry(room_id.to_owned())
            .or_insert_with(RoomActivity::new);
        room.waits.entry(participant_id.to_owned()).or_default().0 += 1;
        room.active
            .send_if_modified(|active| active.listening.insert(participant_id.to_owned()));
        ListeningWait {
            activity: self.clone(),
            room_id: room_id.to_owned(),
            participant_id: participant_id.to_owned(),
        }
    }

    fn stop_listening(&self, room_id: &str, participant_id: &str, generation: u64) {
        let mut rooms = self
            .rooms
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(room) = rooms.get_mut(room_id) else {
            return;
        };
        if room.waits.get(participant_id) != Some(&(0, generation)) {
            return;
        }
        room.waits.remove(participant_id);
        room.active
            .send_if_modified(|active| active.listening.remove(participant_id));
        if room.unused() {
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
        assert!(active.borrow_and_update().working.contains("connector-a"));
        activity.end("room", "connector-a");
        assert!(active.borrow_and_update().working.is_empty());
        activity.start("room", "connector-a");
        tokio::time::advance(super::MAX_ACTIVE / 2).await;
        // A later start keeps the work alive past the first start's deadline.
        activity.start("room", "connector-a");
        tokio::time::advance(super::MAX_ACTIVE / 2 + std::time::Duration::from_secs(1)).await;
        tokio::task::yield_now().await;
        assert!(active.borrow_and_update().working.contains("connector-a"));
        tokio::time::advance(super::MAX_ACTIVE).await;
        tokio::task::yield_now().await;
        assert!(active.borrow_and_update().working.is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn a_pending_wait_listens_and_a_quick_rewait_does_not_flicker() {
        let activity = ParticipantActivity::default();
        let mut active = activity.subscribe("room");
        let first = activity.listen("room", "connector-a");
        assert!(active.borrow_and_update().listening.contains("connector-a"));
        drop(first);
        tokio::task::yield_now().await;
        tokio::time::advance(super::LISTEN_GRACE / 2).await;
        let second = activity.listen("room", "connector-a");
        tokio::time::advance(super::LISTEN_GRACE).await;
        tokio::task::yield_now().await;
        assert!(active.borrow().listening.contains("connector-a"));
        drop(second);
        // Let the grace timer register before the clock moves.
        tokio::task::yield_now().await;
        tokio::time::advance(super::LISTEN_GRACE + std::time::Duration::from_secs(1)).await;
        tokio::task::yield_now().await;
        assert!(active.borrow_and_update().listening.is_empty());
    }
}
