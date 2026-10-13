//! A human download retains provenance, never a database connection.
use super::{MessageAttachmentMetadata, invalid_attachment_state, read_bound_message_metadata};
use crate::{PersistenceError, RoomSessionAuthorization, SqliteStore};
use sqlx::{Sqlite, Transaction};

/// Exact transport-admitted provenance, revalidated for each piece.
pub enum MessageAttachmentReadAuthority {
    Local {
        room_id: String,
        user_id: String,
        participant_id: String,
    },
    Session(Box<RoomSessionAuthorization>),
    Provider(Box<crate::ProviderFileAuthority>),
    /// Attendee downloads keep their connection custody alongside the turn's
    /// provider metadata authority; both are revalidated for each piece.
    Attendee {
        attendee: Box<crate::AttendeeFileAuthority>,
        provider: Box<crate::ProviderFileAuthority>,
    },
    Connector(Box<crate::ConnectorFileAuthority>),
}

/// Immutable response metadata and authority; each read uses a new short transaction.
pub struct MessageAttachmentReader {
    store: SqliteStore,
    authority: MessageAttachmentReadAuthority,
    metadata: MessageAttachmentMetadata,
    original: MessageAttachmentMetadata,
    preview: bool,
}

impl MessageAttachmentReadAuthority {
    async fn metadata_in(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        id: &str,
    ) -> Result<(MessageAttachmentMetadata, String), PersistenceError> {
        if let Self::Provider(authority) = self {
            let metadata = super::provider_attachment_metadata_in(
                tx,
                authority.read_authority(),
                id,
                agentsassemble_domain::MAX_ROOM_FILE_BYTES,
            )
            .await?;
            return Ok((metadata, authority.session_id.clone()));
        }
        if let Self::Attendee { attendee, provider } = self {
            let principal = attendee.resolve(tx).await?;
            let metadata = super::provider_attachment_metadata_in(
                tx,
                provider.read_authority(),
                id,
                agentsassemble_domain::MAX_ROOM_FILE_BYTES,
            )
            .await?;
            return Ok((metadata, principal.principal_id));
        }
        if let Self::Connector(authority) = self {
            // Saved connector reads follow the shared reader policy: already-bound
            // files up to 50 MiB stream even when the upload policy was lowered.
            let principal = authority.read_principal(tx).await?;
            let metadata = read_bound_message_metadata(
                tx,
                &principal.room_id,
                id,
                agentsassemble_domain::MAX_ROOM_FILE_BYTES,
            )
            .await?;
            return Ok((metadata, principal.principal_id.clone()));
        }
        let (room_id, principal_id) = match self {
            Self::Provider(_) | Self::Attendee { .. } | Self::Connector(_) => {
                unreachable!("early return above")
            }
            Self::Local {
                room_id,
                user_id,
                participant_id,
            } => {
                crate::room_user_identity::resolve_room_user_identity(
                    tx,
                    room_id,
                    user_id,
                    participant_id,
                )
                .await?;
                (room_id.clone(), user_id.clone())
            }
            Self::Session(session) => {
                let principal = session.mutation_authority().resolve(tx).await?;
                if !principal.capabilities.room_history {
                    return Err(super::rejected(
                        "permission_denied",
                        "This room session cannot read message history.",
                    ));
                }
                (principal.room_id.clone(), principal.principal_id.clone())
            }
        };
        let metadata = read_bound_message_metadata(
            tx,
            &room_id,
            id,
            agentsassemble_domain::MAX_ROOM_FILE_BYTES,
        )
        .await?;
        if let Self::Session(session) = self {
            session.mutation_authority().record_success(tx).await?;
        }
        Ok((metadata, principal_id))
    }
}

impl SqliteStore {
    /// Authorize metadata and admit before reading any file bytes.
    ///
    /// # Errors
    /// Rejects inaccessible/noncanonical files or unavailable admission.
    pub async fn open_message_attachment_reader<T>(
        &self,
        authority: MessageAttachmentReadAuthority,
        id: &str,
        admit: impl FnOnce(&str) -> Result<T, PersistenceError>,
    ) -> Result<(MessageAttachmentReader, T), PersistenceError> {
        if !agentsassemble_domain::is_message_attachment_id(id) {
            return Err(super::message_attachment_missing());
        }
        let mut tx = self.pool.begin().await?;
        let (metadata, principal) = authority.metadata_in(&mut tx, id).await?;
        let lease = admit(&principal)?;
        tx.commit().await?;
        Ok((
            MessageAttachmentReader {
                store: self.clone(),
                authority,
                original: metadata.clone(),
                metadata,
                preview: false,
            },
            lease,
        ))
    }
}

impl MessageAttachmentReader {
    #[must_use]
    pub const fn metadata(&self) -> &MessageAttachmentMetadata {
        &self.metadata
    }

    /// Read exactly one authorized 64 KiB piece (or its shorter final piece).
    /// The transaction/connection is released before returning to a network owner.
    ///
    /// # Errors
    /// Any lost authority, deletion, changed metadata or missing/corrupt piece is an error.
    pub async fn read_piece(&self, sequence: usize) -> Result<Vec<u8>, PersistenceError> {
        let mut tx = self.store.pool.begin().await?;
        let (metadata, _) = self
            .authority
            .metadata_in(&mut tx, &self.metadata.id)
            .await?;
        if metadata != self.original {
            return Err(invalid_attachment_state());
        }
        let bytes = if self.preview {
            self.preview_piece(&mut tx, sequence).await?
        } else {
            crate::room_file_content::RoomFileContent::Message
                .read(&mut tx, &metadata.id, sequence)
                .await?
        };
        tx.commit().await?;
        Ok(bytes)
    }
}

#[cfg(test)]
#[path = "message_attachment_reader_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "message_attachment_preview_reader_tests.rs"]
mod preview_tests;
#[path = "message_attachment_preview_reader.rs"]
mod previews;
