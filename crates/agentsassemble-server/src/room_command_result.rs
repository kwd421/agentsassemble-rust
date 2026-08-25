use agentsassemble_persistence::PersistenceError;
use agentsassemble_protocol::CommandResolution;

#[derive(Debug)]
pub(crate) struct CommandFailure {
    pub(crate) error: PersistenceError,
    pub(crate) resolution: CommandResolution,
}

impl CommandFailure {
    pub(crate) fn rejected(error: PersistenceError) -> Self {
        Self {
            error,
            resolution: CommandResolution::Rejected,
        }
    }

    pub(crate) fn unresolved(error: PersistenceError) -> Self {
        Self {
            error,
            resolution: CommandResolution::Unresolved,
        }
    }

    pub(crate) fn transactional(error: PersistenceError) -> Self {
        if matches!(
            error,
            PersistenceError::CommandConflict
                | PersistenceError::CommandRejected { .. }
                | PersistenceError::StoredCommandRejected { .. }
        ) {
            Self::rejected(error)
        } else {
            Self::unresolved(error)
        }
    }

    pub(crate) fn after_admission(error: PersistenceError) -> Self {
        if matches!(error, PersistenceError::CommandConflict)
            || matches!(
                error,
                PersistenceError::CommandRejected {
                    code: "write_budget_exceeded",
                    ..
                }
            )
        {
            Self::rejected(error)
        } else {
            Self::unresolved(error)
        }
    }
}
