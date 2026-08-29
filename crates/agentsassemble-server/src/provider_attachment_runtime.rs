use agentsassemble_persistence::{PersistenceError, ProviderAttachmentReadAuthority, SqliteStore};
use agentsassemble_provider::{
    ProviderAttachment, ProviderAttachmentReadCommand, ProviderAttachmentReadError,
};

pub(crate) async fn handle_provider_attachment_read(
    store: &SqliteStore,
    room_id: &str,
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
        )
        .await
        .map(|attachment| ProviderAttachment {
            id: attachment.metadata.id,
            filename: attachment.metadata.filename,
            content_type: attachment.metadata.content_type,
            size: attachment.metadata.size,
            is_image: attachment.metadata.is_image,
            content: attachment.content,
        })
        .map_err(public_read_error);
    command.complete(result);
}

fn public_read_error(error: PersistenceError) -> ProviderAttachmentReadError {
    match error {
        PersistenceError::CommandRejected { code, message }
            if matches!(code, "message_attachment_missing" | "stale_provider_turn") =>
        {
            ProviderAttachmentReadError { code, message }
        }
        _ => ProviderAttachmentReadError {
            code: "persistence_error",
            message: "The room attachment could not be read.".to_owned(),
        },
    }
}
