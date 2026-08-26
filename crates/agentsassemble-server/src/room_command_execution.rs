use agentsassemble_domain::RoomEvent;
use agentsassemble_persistence::{
    AgentTurnAssignment, AgentTurnCommit, CommandOutcome, ParticipantLeaveMutation,
    ParticipantMuteMutation, PersistenceError, RoomCommandMutation, SqliteStore,
};
use agentsassemble_protocol::CommandResolution;

use crate::room_command_result::CommandFailure;

pub(crate) struct CommandExecution {
    pub(crate) reply: Result<CommandOutcome, CommandFailure>,
    pub(crate) committed_events: Vec<RoomEvent>,
    pub(crate) assignments: Vec<AgentTurnAssignment>,
    pub(crate) revoked_human_sessions: Vec<[u8; 32]>,
}

impl CommandExecution {
    pub(crate) fn is_definitive(&self) -> bool {
        match &self.reply {
            Ok(_) => true,
            Err(failure) => failure.resolution != CommandResolution::Unresolved,
        }
    }

    pub(crate) fn success(outcome: CommandOutcome) -> Self {
        let committed_events = if outcome.deduplicated {
            Vec::new()
        } else {
            outcome.events.clone()
        };
        Self {
            reply: Ok(outcome),
            committed_events,
            assignments: Vec::new(),
            revoked_human_sessions: Vec::new(),
        }
    }

    pub(crate) fn mutation(mutation: RoomCommandMutation) -> Self {
        let committed_events = if mutation.outcome.deduplicated {
            Vec::new()
        } else {
            mutation.outcome.events.clone()
        };
        Self {
            reply: Ok(mutation.outcome),
            committed_events,
            assignments: mutation.assignments,
            revoked_human_sessions: Vec::new(),
        }
    }

    pub(crate) fn participant_mute(mutation: ParticipantMuteMutation) -> Self {
        let committed_events = if mutation.outcome.deduplicated {
            Vec::new()
        } else {
            mutation.outcome.events.clone()
        };
        Self {
            reply: Ok(mutation.outcome),
            committed_events,
            assignments: mutation.assignments,
            revoked_human_sessions: Vec::new(),
        }
    }

    pub(crate) fn participant_leave(mutation: ParticipantLeaveMutation) -> Self {
        let committed_events = mutation.outcome.events.clone();
        Self {
            reply: Ok(mutation.outcome),
            committed_events,
            assignments: Vec::new(),
            revoked_human_sessions: mutation.revoked_session_fingerprints,
        }
    }

    pub(crate) fn extend_turn_commit(&mut self, commit: AgentTurnCommit) {
        self.committed_events.extend(commit.events);
        self.assignments.extend(commit.next_assignments);
    }

    pub(crate) fn failure(failure: CommandFailure) -> Self {
        Self {
            reply: Err(failure),
            committed_events: Vec::new(),
            assignments: Vec::new(),
            revoked_human_sessions: Vec::new(),
        }
    }

    pub(crate) fn transactional_failure(error: PersistenceError) -> Self {
        Self::failure(CommandFailure::transactional(error))
    }

    pub(crate) fn unresolved_failure(error: PersistenceError) -> Self {
        Self {
            reply: Err(CommandFailure::unresolved(error)),
            committed_events: Vec::new(),
            assignments: Vec::new(),
            revoked_human_sessions: Vec::new(),
        }
    }

    pub(crate) fn unresolved_failure_with_events(
        error: PersistenceError,
        committed_events: Vec<RoomEvent>,
    ) -> Self {
        Self {
            reply: Err(CommandFailure::unresolved(error)),
            committed_events,
            assignments: Vec::new(),
            revoked_human_sessions: Vec::new(),
        }
    }

    pub(crate) fn committed_failure(
        error: PersistenceError,
        committed_events: Vec<RoomEvent>,
    ) -> Self {
        Self {
            reply: Err(CommandFailure::rejected(error)),
            committed_events,
            assignments: Vec::new(),
            revoked_human_sessions: Vec::new(),
        }
    }
}

pub(crate) async fn progressed_execution(
    store: &SqliteStore,
    room_id: &str,
    outcome: CommandOutcome,
) -> CommandExecution {
    progress_execution(store, room_id, CommandExecution::success(outcome)).await
}

pub(crate) async fn progress_execution(
    store: &SqliteStore,
    room_id: &str,
    mut execution: CommandExecution,
) -> CommandExecution {
    match store.assign_pending_turn(room_id).await {
        Ok(Some(commit)) => {
            execution.committed_events.extend(commit.events);
            execution.assignments.extend(commit.next_assignments);
        }
        Ok(None) => {}
        Err(error) => {
            let code = match &error {
                PersistenceError::CommandRejected { code, .. } => *code,
                _ => "persistence_error",
            };
            tracing::error!(
                code,
                room_id,
                "committed lifecycle command could not advance the ordered floor"
            );
            match store.record_floor_progression_failure(room_id, code).await {
                Ok(events) => execution.committed_events.extend(events),
                Err(recording_error) => tracing::error!(
                    error = ?recording_error,
                    room_id,
                    "room floor progression failure could not be recorded durably"
                ),
            }
        }
    }
    execution
}

pub(crate) fn persistence_error_code(error: &PersistenceError) -> &'static str {
    match error {
        PersistenceError::CommandRejected { code, .. }
        | PersistenceError::CommandUnresolved { code, .. } => code,
        PersistenceError::CommandConflict => "command_conflict",
        PersistenceError::StoredCommandRejected { .. } => "stored_command_rejected",
        PersistenceError::Database(_) => "database_error",
        PersistenceError::Json(_) => "stored_json_invalid",
        PersistenceError::RuntimeAuthorityTask(_) => "runtime_authority_task_failed",
        PersistenceError::AuthorityConflict(_) => "authority_conflict",
        PersistenceError::ParticipantMissing => "participant_missing",
        _ => "persistence_error",
    }
}
