use sqlx::{Sqlite, Transaction};

use crate::{OperatorSessionAuthorization, PersistenceError};

/// Transport-authenticated server owner; remote provenance is checked again in storage.
/// Local callers must first consume their native one-use operator ticket.
pub enum ServerOwnerAuthority {
    LocalOperator,
    CentralOwner(Box<OperatorSessionAuthorization>),
}

impl ServerOwnerAuthority {
    pub(crate) async fn revalidate(
        &self,
        transaction: &mut Transaction<'_, Sqlite>,
    ) -> Result<(), PersistenceError> {
        if let Self::CentralOwner(session) = self {
            crate::operator_pairing::revalidate_central_owner_session(transaction, session).await?;
        }
        crate::bootstrap::require_complete_bootstrap_in_transaction(transaction).await?;
        Ok(())
    }
}
