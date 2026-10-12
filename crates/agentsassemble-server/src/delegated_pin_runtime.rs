use agentsassemble_domain::{SetMessagePin, canonical_payload_hash};
use agentsassemble_persistence::{
    CommandOutcome, DelegateRoomAuthority, PersistenceError, SqliteStore,
};
use serde_json::json;

use super::RoomRuntime;
use crate::principal_mutation_admission::{MutationIdentity, PrincipalMutationAdmission};

impl RoomRuntime {
    /// Executes a pin with current AI/owner custody and the shared principal write budgets.
    /// # Errors
    /// Propagates authority, admission, target and persistence failures without publishing success.
    pub async fn set_delegated_message_pin(
        &self,
        authority: &DelegateRoomAuthority,
        request: &SetMessagePin,
    ) -> Result<CommandOutcome, PersistenceError> {
        let outcome = apply(&self.store, &self.principal_mutations, authority, request).await?;
        if !outcome.deduplicated {
            self.notify_room_publication(authority.room_id()).await;
        }
        Ok(outcome)
    }
}

pub(crate) async fn apply(
    store: &SqliteStore,
    admission: &PrincipalMutationAdmission,
    authority: &DelegateRoomAuthority,
    request: &SetMessagePin,
) -> Result<CommandOutcome, PersistenceError> {
    let _inflight = admission.acquire_inflight()?;
    let (outcome, debits) = store
        .set_delegated_message_pin(authority, request, |owner, delegate, bytes| {
            // Different AIs' identical UUIDs must not share one owner's debit.
            let hash = canonical_payload_hash(&json!({"delegate":delegate,"pin":request}));
            let identity = MutationIdentity::new(
                authority.room_id(),
                &request.request_id,
                "set_message_pin",
                &hash,
            );
            Ok([
                admission.charge(owner, identity, bytes)?,
                admission.charge(delegate, identity, bytes)?,
            ])
        })
        .await?;
    if let Some(debits) = debits {
        for debit in debits {
            debit.resolve();
        }
    }
    Ok(outcome)
}

#[cfg(test)]
#[path = "delegated_pin_runtime_tests.rs"]
mod tests;
