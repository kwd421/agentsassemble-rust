use agentsassemble_domain::{AuthenticatedPrincipal, canonical_payload_hash};
use agentsassemble_persistence::{
    RoomMutationAuthority, RoomSessionAuthorization, SideChatCommit, room_write_command_size,
};
use agentsassemble_protocol::{CommandResolution, RoomAction};
use serde_json::Value;

use crate::{
    principal_mutation_admission::MutationIdentity,
    room_command_result::{CommandFailure, validate_command_envelope},
};

impl super::RoomRuntime {
    pub(crate) async fn execute_side_chat(
        &self,
        principal: &AuthenticatedPrincipal,
        session: Option<&RoomSessionAuthorization>,
        request_id: &str,
        payload: &Value,
    ) -> Result<SideChatCommit, CommandFailure> {
        validate_command_envelope(request_id).map_err(CommandFailure::rejected)?;
        let action = RoomAction::SideChatSend.as_str();
        let bytes = room_write_command_size(request_id, action, payload)
            .map_err(CommandFailure::rejected)?;
        let hash = canonical_payload_hash(payload);
        let debit = self
            .principal_mutations
            .charge(
                &principal.principal_id,
                MutationIdentity::new(&principal.room_id, request_id, action, &hash),
                bytes,
            )
            .map_err(CommandFailure::after_admission)?;
        let _inflight = self
            .principal_mutations
            .acquire_inflight()
            .map_err(CommandFailure::after_admission)?;
        let authority = session.map_or(
            RoomMutationAuthority::TrustedPrincipal(principal),
            RoomSessionAuthorization::mutation_authority,
        );
        let result = self
            .store
            .execute_side_chat(authority, request_id, payload, chrono::Utc::now())
            .await
            .map_err(CommandFailure::transactional);
        if matches!(
            &result,
            Ok(_)
                | Err(CommandFailure {
                    resolution: CommandResolution::Rejected,
                    ..
                })
        ) {
            debit.resolve();
        }
        result
    }
}
