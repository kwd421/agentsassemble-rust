use agentsassemble_persistence::{PersistenceError, ProviderAttachmentReadAuthority, SqliteStore};
use agentsassemble_provider::{
    ProviderAttachment, ProviderAttachmentReadCommand, ProviderAttachmentReadError,
};

pub(crate) async fn handle_provider_attachment_read(
    store: &SqliteStore,
    room_id: &str,
    downloads: &crate::download_budget::Budget,
    command: ProviderAttachmentReadCommand,
) {
    let result = store
        .bound_provider_message_attachment(
            ProviderAttachmentReadAuthority {
                room_id,
                session_id: command.session_id(),
                turn_id: command.turn_id(),
                input_up_to_seq: command.input_up_to_seq(),
                turn_generation: command.turn_generation(),
                execution_id: command.execution_id(),
            },
            command.attachment_id(),
            |principal, size| downloads.reserve_encoded(principal, size),
        )
        .await
        .map(|(attachment, lease)| {
            provider_attachment(attachment.metadata, lease.retain(attachment.content))
        })
        .map_err(public_read_error);
    command.complete(result);
}

pub(crate) fn into_provider_attachment(
    attachment: agentsassemble_persistence::MessageAttachment,
) -> ProviderAttachment {
    provider_attachment(attachment.metadata, attachment.content.into())
}

fn provider_attachment(
    attachment: agentsassemble_persistence::MessageAttachmentMetadata,
    content: axum::body::Bytes,
) -> ProviderAttachment {
    ProviderAttachment {
        id: attachment.id,
        filename: attachment.filename,
        content_type: attachment.content_type,
        size: attachment.size,
        is_image: attachment.is_image,
        content,
    }
}

fn public_read_error(error: PersistenceError) -> ProviderAttachmentReadError {
    match error {
        PersistenceError::CommandRejected { code, message }
            if matches!(
                code.as_bytes(),
                b"message_attachment_missing"
                    | b"stale_provider_turn"
                    | b"attachment_too_large"
                    | b"attachment_download_budget_exhausted"
            ) =>
        {
            ProviderAttachmentReadError { code, message }
        }
        _ => ProviderAttachmentReadError {
            code: "persistence_error".into(),
            message: "The room attachment could not be read.".to_owned(),
        },
    }
}
