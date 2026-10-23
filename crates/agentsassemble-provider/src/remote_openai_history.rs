//! The room conversation an API agent carries from turn to turn. Each turn
//! appends its request, the room messages it had not seen and its own work,
//! so every request repeats the previous one word for word and adds to its
//! end: the provider serves that shared beginning from its prompt cache.
//! In-turn compaction (`turn_compaction`) folds the oldest part when the
//! conversation grows past its window.
use std::borrow::Cow;

use agentsassemble_domain::DurableAgentSession;
use serde_json::{Value, json};

use super::{RemoteOpenAiDriver, opening_messages, system_message};
use crate::{driver::ProviderTurnRequest, turn_compaction};

/// A carried conversation is compacted within this window whatever the model
/// allows: every turn resends it, so its size is each turn's cost.
pub(super) const ROOM_CONVERSATION_TOKENS: u64 = 48_000;

pub(super) struct ConversationHistory {
    messages: Vec<Value>,
    /// The input the last completed turn answered; the next turn's room view
    /// must start right after it.
    synced_seq: i64,
    /// The provider's usage report for `messages[..covered]`.
    measured: Option<(usize, u64)>,
}

impl ConversationHistory {
    /// The server sent the messages since exactly the turn this conversation
    /// holds, so the turn can continue it.
    pub(super) fn continues(history: Option<&Self>, request: &ProviderTurnRequest) -> bool {
        history.is_some_and(|history| {
            request
                .room_observation
                .as_ref()
                .and_then(|observation| observation.view_delta.as_ref())
                .is_some_and(|delta| delta.since_seq == history.synced_seq)
        })
    }

    /// The messages to go on from, with this turn's room rules and request,
    /// and where compaction may start folding.
    pub(super) fn resume(
        self,
        system: Option<Value>,
        input: &str,
    ) -> (Vec<Value>, usize, Option<(usize, u64)>) {
        let Self {
            mut messages,
            measured,
            ..
        } = self;
        let had_system = messages
            .first()
            .is_some_and(|first| first["role"] == "system");
        let head = usize::from(had_system);
        // Rules can change between turns (instructions, card, permission); a
        // new head costs one uncached request, not the conversation.
        let unchanged = messages[..head] == *system.as_slice();
        if !unchanged {
            messages.splice(..head, system);
        }
        messages.push(json!({"role": "user", "content": input}));
        let opening = usize::from(
            messages
                .first()
                .is_some_and(|first| first["role"] == "system"),
        );
        (messages, opening, measured.filter(|_| unchanged))
    }
}

/// Where a turn starts.
pub(super) struct TurnStart {
    pub(super) messages: Vec<Value>,
    /// Compaction folds only what comes after this.
    pub(super) opening: usize,
    /// The provider's usage report for `messages[..covered]`.
    pub(super) measured: Option<(usize, u64)>,
    /// The window compaction keeps the messages in.
    pub(super) context_tokens: u64,
}

impl RemoteOpenAiDriver {
    /// A room turn continues the conversation the agent carries, or opens a
    /// new one from the full window. A failed turn leaves none, so the next
    /// opens fresh.
    pub(super) fn turn_start(
        &mut self,
        session: &DurableAgentSession,
        request: &ProviderTurnRequest,
        memory: Option<&str>,
    ) -> TurnStart {
        let room = request.room_observation.is_some();
        let continues = room && ConversationHistory::continues(self.history.as_ref(), request);
        let held = if room { self.history.take() } else { None }.filter(|_| continues);
        let (messages, opening, measured) = if let Some(history) = held {
            history.resume(system_message(session, request), &request.input)
        } else {
            let messages = opening_messages(session, request, memory);
            let opening = messages.len();
            (messages, opening, None)
        };
        let context_tokens = request
            .context_tokens
            .unwrap_or(turn_compaction::UNKNOWN_CONTEXT_TOKENS)
            .min(if room {
                ROOM_CONVERSATION_TOKENS
            } else {
                u64::MAX
            });
        TurnStart {
            messages,
            opening,
            measured,
            context_tokens,
        }
    }

    /// A completed room turn's conversation is what the next one continues.
    pub(super) fn keep_conversation(
        &mut self,
        request: &ProviderTurnRequest,
        messages: Vec<Value>,
        measured: Option<(usize, u64)>,
    ) {
        if let Some(observation) = request.room_observation.as_ref() {
            self.history = Some(ConversationHistory {
                messages,
                synced_seq: observation.input_up_to_seq,
                measured,
            });
        }
    }
}

/// What the room portal serves this turn: a continued conversation reads only
/// the room messages it has not seen.
pub(super) fn portal_request(
    request: &ProviderTurnRequest,
    continues: bool,
) -> Cow<'_, ProviderTurnRequest> {
    let delta = request
        .room_observation
        .as_ref()
        .and_then(|observation| observation.view_delta.as_ref())
        .filter(|_| continues);
    let Some(delta) = delta else {
        return Cow::Borrowed(request);
    };
    let mut request = request.clone();
    if let Some(observation) = request.room_observation.as_mut() {
        observation.view.clone_from(&delta.view);
        // Attachments listed for this turn must appear in the view it reads.
        observation
            .attachment_ids
            .retain(|attachment_id| delta.view.contains(attachment_id.as_str()));
    }
    Cow::Owned(request)
}
