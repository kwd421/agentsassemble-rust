use std::collections::{HashMap, VecDeque};

use agentsassemble_domain::{
    AuthenticatedPrincipal, ClientKind, Participant, SIDE_CHAT_MAX_MESSAGES, SIDE_CHAT_TTL_SECONDS,
    SideChatMessage, SideChatSend, SideChatSnapshot, SideChatUpdate, canonical_payload_hash,
    require_message_write_authority,
};
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::{Sqlite, Transaction};
use tokio::sync::{Mutex, broadcast};
use uuid::Uuid;

use crate::{
    PersistenceError, RoomMutationAuthority, SqliteStore, authority::load_active_membership,
};

#[derive(Default)]
pub(crate) struct SideChatRepository {
    rooms: Mutex<HashMap<Uuid, SideChatRoom>>,
}

impl SideChatRepository {
    pub(crate) async fn clear_room(&self, room_uid: Uuid) {
        self.rooms.lock().await.remove(&room_uid);
    }
}

struct SideChatRoom {
    generation: Uuid,
    latest_seq: i64,
    retained_after_seq: i64,
    messages: VecDeque<RetainedMessage>,
    updates: broadcast::Sender<SideChatUpdate>,
}

struct RetainedMessage {
    principal_id: String,
    request_id: String,
    payload_hash: String,
    message: SideChatMessage,
}

impl Default for SideChatRoom {
    fn default() -> Self {
        Self {
            generation: Uuid::new_v4(),
            latest_seq: 0,
            retained_after_seq: 0,
            messages: VecDeque::new(),
            updates: broadcast::channel(SIDE_CHAT_MAX_MESSAGES).0,
        }
    }
}

impl SideChatRoom {
    fn prune(&mut self, now: DateTime<Utc>) {
        let cutoff = now - chrono::Duration::seconds(SIDE_CHAT_TTL_SECONDS);
        while self
            .messages
            .front()
            .is_some_and(|entry| entry.message.created_at <= cutoff)
            || self.messages.len() > SIDE_CHAT_MAX_MESSAGES
        {
            if let Some(expired) = self.messages.pop_front() {
                self.retained_after_seq = expired.message.seq;
            }
        }
    }

    fn snapshot(&self, room_id: String) -> SideChatSnapshot {
        SideChatSnapshot {
            room_id,
            generation: self.generation,
            retained_after_seq: self.retained_after_seq,
            latest_seq: self.latest_seq,
            messages: self
                .messages
                .iter()
                .map(|entry| entry.message.clone())
                .collect(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SideChatCommit {
    pub update: SideChatUpdate,
    pub deduplicated: bool,
}

impl SqliteStore {
    /// Reads only the current human room's ephemeral chat, never durable room events.
    ///
    /// # Errors
    /// Rejects stale/non-human authority, absent read permission and database failures.
    pub async fn side_chat_snapshot(
        &self,
        authority: RoomMutationAuthority<'_>,
        now: DateTime<Utc>,
    ) -> Result<SideChatSnapshot, PersistenceError> {
        let mut tx = self.pool.begin().await?;
        let (room_uid, principal, _) = human_authority(&mut tx, authority, false).await?;
        let snapshot = {
            let mut rooms = self.side_chat.rooms.lock().await;
            let room = rooms.entry(room_uid).or_default();
            room.prune(now);
            room.snapshot(principal.room_id)
        };
        tx.commit().await?;
        Ok(snapshot)
    }

    /// Registers live delivery before the caller obtains its HTTP/bootstrap snapshot.
    ///
    /// # Errors
    /// Rejects stale/non-human authority or database failures before subscribing.
    pub async fn subscribe_side_chat(
        &self,
        authority: RoomMutationAuthority<'_>,
    ) -> Result<broadcast::Receiver<SideChatUpdate>, PersistenceError> {
        let mut tx = self.pool.begin().await?;
        let (room_uid, _, _) = human_authority(&mut tx, authority, false).await?;
        let receiver = self
            .side_chat
            .rooms
            .lock()
            .await
            .entry(room_uid)
            .or_default()
            .updates
            .subscribe();
        tx.commit().await?;
        Ok(receiver)
    }

    /// Appends one human message in memory while the single `SQLite` connection holds current authority.
    ///
    /// # Errors
    /// Rejects read-only/muted/non-human callers, stale generations, conflicting retries
    /// or an expired retry horizon. A read-transaction completion failure remains unresolved.
    pub async fn execute_side_chat(
        &self,
        authority: RoomMutationAuthority<'_>,
        request_id: &str,
        payload: &Value,
        now: DateTime<Utc>,
    ) -> Result<SideChatCommit, PersistenceError> {
        let request = SideChatSend::from_payload(payload).map_err(rejection)?;
        let mut tx = self.pool.begin().await?;
        let (room_uid, principal, participant) = human_authority(&mut tx, authority, true).await?;
        let outcome = {
            let mut rooms = self.side_chat.rooms.lock().await;
            let room = rooms.entry(room_uid).or_default();
            room.prune(now);
            append(
                room,
                &principal,
                &participant,
                request_id,
                payload,
                request,
                now,
            )?
        };
        // This transaction only read authority. The commit point was the in-memory
        // append; a lost connection here cannot undo it, and its exact receipt survives.
        tx.commit()
            .await
            .map_err(|_| PersistenceError::CommandUnresolved {
                code: "side_chat_result_uncertain",
                message: "The side-chat result is uncertain; retry the same request.".to_owned(),
            })?;
        Ok(outcome)
    }
}

fn append(
    room: &mut SideChatRoom,
    principal: &AuthenticatedPrincipal,
    participant: &Participant,
    request_id: &str,
    payload: &Value,
    request: SideChatSend,
    now: DateTime<Utc>,
) -> Result<SideChatCommit, PersistenceError> {
    if request.generation != room.generation || request.after_seq > room.latest_seq {
        return Err(expired_retry());
    }
    let hash = canonical_payload_hash(payload);
    if let Some(previous) = room.messages.iter().find(|entry| {
        entry.principal_id == principal.principal_id && entry.request_id == request_id
    }) {
        if previous.payload_hash != hash {
            return Err(PersistenceError::CommandConflict);
        }
        return Ok(SideChatCommit {
            update: SideChatUpdate {
                room_id: principal.room_id.clone(),
                generation: room.generation,
                retained_after_seq: room.retained_after_seq,
                message: previous.message.clone(),
            },
            deduplicated: true,
        });
    }
    if request.after_seq < room.retained_after_seq {
        return Err(expired_retry());
    }
    let sequence = room.latest_seq.checked_add(1).ok_or_else(expired_retry)?;
    let message = SideChatMessage {
        id: Uuid::new_v4(),
        seq: sequence,
        created_at: now,
        participant_id: participant.participant_id.clone(),
        display_name: participant.display_name.clone(),
        content: request.content,
    };
    room.messages.push_back(RetainedMessage {
        principal_id: principal.principal_id.clone(),
        request_id: request_id.to_owned(),
        payload_hash: hash,
        message: message.clone(),
    });
    room.latest_seq = sequence;
    room.prune(now);
    let update = SideChatUpdate {
        room_id: principal.room_id.clone(),
        generation: room.generation,
        retained_after_seq: room.retained_after_seq,
        message,
    };
    // No listeners is normal; a later bootstrap reads the same memory owner.
    let _ = room.updates.send(update.clone());
    Ok(SideChatCommit {
        update,
        deduplicated: false,
    })
}

async fn human_authority(
    tx: &mut Transaction<'_, Sqlite>,
    authority: RoomMutationAuthority<'_>,
    writing: bool,
) -> Result<(Uuid, AuthenticatedPrincipal, Participant), PersistenceError> {
    let principal = authority.resolve(tx).await?.into_owned();
    let (room, participant) =
        load_active_membership(tx, &principal.room_id, &principal.participant_id).await?;
    if principal.client_kind != ClientKind::Browser
        || participant.participant_type != "human"
        || !principal.capabilities.room_history
    {
        return Err(PersistenceError::CommandRejected {
            code: "human_side_chat_required",
            message: "Side chat is available only to current human room sessions.".to_owned(),
        });
    }
    if writing {
        require_message_write_authority(&principal, &participant).map_err(rejection)?;
    }
    Ok((room.room_uid, principal, participant))
}

fn expired_retry() -> PersistenceError {
    PersistenceError::CommandUnresolved {
        code: "side_chat_retry_expired",
        message:
            "This side-chat lifetime or retry window ended. Refresh before starting a new message."
                .to_owned(),
    }
}

fn rejection(error: agentsassemble_domain::CommandRejection) -> PersistenceError {
    PersistenceError::CommandRejected {
        code: error.code,
        message: error.message,
    }
}

#[cfg(test)]
#[path = "side_chat_tests.rs"]
mod tests;
