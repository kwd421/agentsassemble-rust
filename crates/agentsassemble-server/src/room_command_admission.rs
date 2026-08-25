use agentsassemble_domain::{AuthenticatedPrincipal, canonical_payload_hash};
use agentsassemble_persistence::{PersistenceError, SqliteStore, room_write_command_size};
use agentsassemble_protocol::RoomAction;
use serde_json::Value;
use tokio::sync::OwnedSemaphorePermit;

use crate::{
    principal_mutation_admission::{MutationDebit, MutationIdentity, PrincipalMutationAdmission},
    room_command_result::{CommandFailure, validate_command_envelope},
};

pub(crate) struct AdmittedHumanCommand {
    pub(crate) principal: AuthenticatedPrincipal,
    pub(crate) mutation_debit: Option<MutationDebit>,
    pub(crate) inflight_permit: OwnedSemaphorePermit,
}

pub(crate) async fn admit_human_command(
    store: &SqliteStore,
    admission: &PrincipalMutationAdmission,
    principal: &AuthenticatedPrincipal,
    request_id: &str,
    action: RoomAction,
    payload: &Value,
) -> Result<AdmittedHumanCommand, CommandFailure> {
    validate_command_envelope(request_id).map_err(CommandFailure::rejected)?;
    let principal = store
        .resolve_principal(principal)
        .await
        .map_err(CommandFailure::unresolved)?;
    let action_name = action.as_str();
    let payload_bytes = room_write_command_size(request_id, action_name, payload)
        .map_err(CommandFailure::unresolved)?;
    let payload_hash = canonical_payload_hash(payload);
    let identity =
        MutationIdentity::new(&principal.room_id, request_id, action_name, &payload_hash);
    let mutation_debit = match store
        .command_requires_principal_budget(&principal, request_id, action_name, payload)
        .await
    {
        Ok(true) => Some(
            admission
                .charge(&principal.principal_id, identity, payload_bytes)
                .map_err(CommandFailure::after_admission)?,
        ),
        Ok(false) => None,
        Err(error) if admission_error_is_definitive(&error) => {
            let debit = admission
                .charge(&principal.principal_id, identity, payload_bytes)
                .map_err(CommandFailure::after_admission)?;
            debit.resolve();
            return Err(CommandFailure::transactional(error));
        }
        Err(error) => return Err(CommandFailure::unresolved(error)),
    };
    let inflight_permit = admission
        .acquire_inflight()
        .map_err(CommandFailure::after_admission)?;
    Ok(AdmittedHumanCommand {
        principal,
        mutation_debit,
        inflight_permit,
    })
}

fn admission_error_is_definitive(error: &PersistenceError) -> bool {
    matches!(
        error,
        PersistenceError::CommandConflict
            | PersistenceError::CommandRejected { .. }
            | PersistenceError::StoredCommandRejected { .. }
            | PersistenceError::ParticipantMissing
            | PersistenceError::RoomMissing
    )
}
