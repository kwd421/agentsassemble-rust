use agentsassemble_persistence::{
    MessageAttachmentReadAuthority, MessageUploadAuthority, PersistenceError,
    ProviderAttachmentReadAuthority, ProviderFileAuthority, SqliteStore,
};
use agentsassemble_provider::{
    ProviderAttachment, ProviderAttachmentReadCommand, ProviderAttachmentReadError,
    ProviderRoomToolCommand, ProviderRoomToolError,
};
use tokio::io::AsyncWriteExt;

pub(crate) async fn handle_provider_attachment_read(
    store: &SqliteStore,
    room_id: &str,
    downloads: &crate::download_budget::Budget,
    mut command: ProviderAttachmentReadCommand,
) {
    let session_id = command.session_id().to_owned();
    let turn_id = command.turn_id().to_owned();
    let execution_id = command.execution_id().to_owned();
    let attachment_id = command.attachment_id().to_owned();
    let authority = ProviderAttachmentReadAuthority {
        room_id,
        session_id: &session_id,
        turn_id: &turn_id,
        input_up_to_seq: command.input_up_to_seq(),
        turn_generation: command.turn_generation(),
        execution_id: &execution_id,
    };
    let result = tokio::select! {
        () = command.cancelled() => return,
        result = read(store, authority, &attachment_id, downloads) => result.map_err(public_read_error),
    };
    command.complete(result);
}

pub(crate) async fn handle_provider_attachment_upload(
    store: &SqliteStore,
    uploads: &crate::message_upload::Service,
    room_id: &str,
    mut command: ProviderRoomToolCommand,
) {
    if let Err(error) = command.begin_execution().await {
        command.complete(Err(error));
        return;
    }
    let agentsassemble_provider::ProviderRoomToolRequest::UploadAttachment { path, content_type } =
        command.request().clone()
    else {
        unreachable!("file owner receives upload requests");
    };
    let session_id = command.session_id().to_owned();
    let turn_id = command.turn_id().to_owned();
    let execution_id = command.execution_id().to_owned();
    let authority = ProviderAttachmentReadAuthority {
        room_id,
        session_id: &session_id,
        turn_id: &turn_id,
        input_up_to_seq: command.input_up_to_seq(),
        turn_generation: command.turn_generation(),
        execution_id: &execution_id,
    };
    let result = tokio::select! {
        () = command.cancelled() => return,
        result = upload_path(store, uploads, authority, &path, &content_type) => result.map(agentsassemble_provider::ProviderRoomToolResult::UploadAttachment),
    };
    command.complete(result);
}

fn file_authority(
    authority: ProviderAttachmentReadAuthority<'_>,
    session: &agentsassemble_domain::DurableAgentSession,
) -> ProviderFileAuthority {
    ProviderFileAuthority {
        room_id: authority.room_id.into(),
        session_id: authority.session_id.into(),
        participant_id: session.public.participant_id.clone(),
        turn_id: authority.turn_id.into(),
        input_up_to_seq: authority.input_up_to_seq,
        turn_generation: authority.turn_generation,
        execution_id: authority.execution_id.into(),
    }
}
async fn read(
    store: &SqliteStore,
    authority: ProviderAttachmentReadAuthority<'_>,
    id: &str,
    downloads: &crate::download_budget::Budget,
) -> Result<ProviderAttachment, PersistenceError> {
    let session = store.provider_file_session(authority).await?;
    let writable = matches!(
        session.public.permission_mode.as_str(),
        "workspace_write" | "full_access"
    );
    // Read-only/native and inline reads retain their original admitted representation.
    let inline = store
        .bound_provider_message_attachment(authority, id, |principal, size| {
            downloads.reserve_encoded(principal, size)
        })
        .await;
    match inline {
        Ok((attachment, lease)) => {
            return Ok(provider_attachment(
                attachment.metadata,
                lease.retain(attachment.content),
            ));
        }
        Err(PersistenceError::CommandRejected { ref code, .. })
            if code == "attachment_too_large" && writable => {}
        Err(error) => return Err(error),
    }
    let (reader, _lease) = store
        .open_message_attachment_reader(
            MessageAttachmentReadAuthority::Provider(Box::new(file_authority(authority, &session))),
            id,
            |principal| downloads.reserve_stream(principal),
        )
        .await?;
    save(reader, &session.workspace, &session.workspace_identity).await
}
async fn save(
    reader: agentsassemble_persistence::MessageAttachmentReader,
    workspace: &str,
    identity: &str,
) -> Result<ProviderAttachment, PersistenceError> {
    let workspace = workspace.to_owned();
    let identity = identity.to_owned();
    let filename = reader.metadata().filename.clone();
    let mut sink = tokio::task::spawn_blocking(move || {
        agentsassemble_provider::WorkspaceAttachmentSink::new(&workspace, &identity, &filename)
    })
    .await
    .map_err(|_| workspace_error())?
    .map_err(|_| workspace_error())?;
    for sequence in 0..reader
        .metadata()
        .size
        .div_ceil(agentsassemble_persistence::MESSAGE_UPLOAD_PIECE_BYTES)
    {
        let piece = reader.read_piece(sequence).await?;
        sink.file
            .write_all(&piece)
            .await
            .map_err(|_| workspace_error())?;
    }
    let path = sink.publish().await.map_err(|_| workspace_error())?;
    let mut result = provider_attachment(reader.metadata().clone(), axum::body::Bytes::new());
    result.path = Some(path);
    Ok(result)
}
fn workspace_error() -> PersistenceError {
    PersistenceError::CommandRejected {
        code: "workspace_file_failed".into(),
        message:
            "The working-folder file operation failed or the path is outside the working folder."
                .into(),
    }
}
async fn upload_path(
    store: &SqliteStore,
    uploads: &crate::message_upload::Service,
    authority: ProviderAttachmentReadAuthority<'_>,
    path: &str,
    content_type: &str,
) -> Result<serde_json::Value, ProviderRoomToolError> {
    let result = async {
        let session = store.provider_file_session(authority).await?;
        if !matches!(
            session.public.permission_mode.as_str(),
            "workspace_write" | "full_access"
        ) {
            return Err(PersistenceError::CommandRejected {
                code: "permission_denied".into(),
                message: "This conversation-only agent has no workspace file access.".into(),
            });
        }
        let custody =
            MessageUploadAuthority::Provider(Box::new(file_authority(authority, &session)));
        let path = path.to_owned();
        let (file, filename, size) = tokio::task::spawn_blocking(move || {
            agentsassemble_provider::open_workspace_attachment(
                &session.workspace,
                &session.workspace_identity,
                &path,
            )
        })
        .await
        .map_err(|_| workspace_error())?
        .map_err(|_| workspace_error())?;
        let metadata = uploads
            .file(
                custody,
                filename,
                content_type.into(),
                size,
                tokio::fs::File::from_std(file),
            )
            .await
            .map_err(upload_error)?;
        serde_json::to_value(metadata).map_err(PersistenceError::from)
    }
    .await;
    result.map_err(crate::provider_room_tool_runtime::public_tool_error)
}
fn upload_error(error: crate::profile_web::ProfileHttpError) -> PersistenceError {
    PersistenceError::CommandRejected {
        code: error.code.into(),
        message: error.message.into(),
    }
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
        path: None,
    }
}
fn public_read_error(error: PersistenceError) -> ProviderAttachmentReadError {
    match error {
        PersistenceError::CommandRejected { code, message } => {
            ProviderAttachmentReadError { code, message }
        }
        _ => ProviderAttachmentReadError {
            code: "persistence_error".into(),
            message: "The room attachment could not be read.".to_owned(),
        },
    }
}
