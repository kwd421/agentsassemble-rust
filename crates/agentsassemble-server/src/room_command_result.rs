use agentsassemble_domain::{
    AuthenticatedPrincipal, CommandRejection, public_event_for_principal,
    public_value_for_principal,
};
use agentsassemble_persistence::{CommandOutcome, PersistenceError};
use agentsassemble_protocol::CommandResolution;

#[derive(Debug)]
pub(crate) struct CommandFailure {
    pub(crate) error: PersistenceError,
    pub(crate) resolution: CommandResolution,
}

impl CommandFailure {
    pub(crate) fn domain_rejected(error: CommandRejection) -> Self {
        Self::rejected(PersistenceError::CommandRejected {
            code: error.code,
            message: error.message,
        })
    }

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

pub(crate) fn validate_command_envelope(request_id: &str) -> Result<(), PersistenceError> {
    if request_id.is_empty() || request_id.chars().count() > 128 {
        return Err(PersistenceError::CommandRejected {
            code: "command_envelope_invalid",
            message: "request_id is invalid.".to_owned(),
        });
    }
    Ok(())
}

pub(crate) fn public_command_outcome(
    principal: &AuthenticatedPrincipal,
    mut outcome: CommandOutcome,
) -> Result<CommandOutcome, PersistenceError> {
    outcome.result = public_value_for_principal(&outcome.result, principal)?;
    outcome.event = public_event_for_principal(&outcome.event, principal);
    outcome.events = outcome
        .events
        .iter()
        .map(|event| public_event_for_principal(event, principal))
        .collect();
    Ok(outcome)
}
