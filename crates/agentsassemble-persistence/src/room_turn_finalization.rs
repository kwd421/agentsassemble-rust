use agentsassemble_domain::{DurableAgentSession, Room, RoomEvent, RoomSettings};
use sqlx::{Sqlite, Transaction};

use crate::{AgentTurnCommit, PersistenceError, agent_lifecycle::save_session};

use super::{
    assign_available_pending, complete_session_state, error_event, rejected, route_message,
    session_state_event, turn_finished_event,
};

pub(super) enum ProviderTurnDisposition<'a> {
    Completed {
        route_first_event: bool,
    },
    Declined {
        reason_code: &'a str,
    },
    Rejected {
        error_code: &'static str,
        message: &'a str,
    },
}

pub(super) struct ProviderTurnFinalization<'a> {
    pub(super) room: &'a Room,
    pub(super) settings: &'a RoomSettings,
    pub(super) turn_id: &'a str,
    pub(super) provider_turn_id: &'a str,
    pub(super) disposition: ProviderTurnDisposition<'a>,
}

impl ProviderTurnFinalization<'_> {
    pub(super) async fn apply(
        self,
        transaction: &mut Transaction<'_, Sqlite>,
        session: &mut DurableAgentSession,
        mut events: Vec<RoomEvent>,
    ) -> Result<AgentTurnCommit, PersistenceError> {
        let (status, reason_code, route_first_event) = match self.disposition {
            ProviderTurnDisposition::Completed { route_first_event } => {
                ("completed", None, route_first_event)
            }
            ProviderTurnDisposition::Declined { reason_code } => {
                ("declined", Some(reason_code), false)
            }
            ProviderTurnDisposition::Rejected {
                error_code,
                message,
            } => {
                events.push(
                    error_event(transaction, session, self.turn_id, error_code, message).await?,
                );
                ("error", Some(error_code), false)
            }
        };
        let input_event_id = session.input_up_to_event_id.clone();
        let input_seq = session.input_up_to_seq;
        let finished = turn_finished_event(
            transaction,
            session,
            self.turn_id,
            status,
            Some(self.provider_turn_id),
            reason_code,
        )
        .await?;
        complete_session_state(session, &input_event_id, input_seq);
        save_session(transaction, session).await?;
        let state = session_state_event(transaction, session).await?;
        if route_first_event {
            let event = events.first().ok_or_else(|| {
                rejected(
                    "stored_turn_authority_invalid",
                    "A routed provider completion has no canonical room event.",
                )
            })?;
            route_message(transaction, self.settings, event).await?;
        }
        events.extend([finished, state]);
        let prepared = assign_available_pending(transaction, self.room, self.settings).await?;
        let mut next_assignments = Vec::with_capacity(prepared.len());
        for item in prepared {
            events.extend(item.events);
            next_assignments.push(item.assignment);
        }
        Ok(AgentTurnCommit {
            events,
            next_assignments,
        })
    }
}
