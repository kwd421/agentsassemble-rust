//! CLI file custody mirrors the attendee client: the connector's own working
//! folder streams through the host transfer owner, and large reads save
//! atomically under `.agentsassemble-attachments` instead of memory.
use agentsassemble_persistence::MESSAGE_UPLOAD_PIECE_BYTES;
use futures_util::StreamExt;
use serde_json::Value;
use tokio::io::{AsyncSeekExt, AsyncWriteExt};

use super::{ConnectorClientError, RoomConnectorClient};

/// The CLI machine's file custody: the folder the CLI was started in.
pub struct ConnectorFileContext {
    pub workspace: String,
    pub identity: String,
}

pub(crate) fn workspace_failed() -> ConnectorClientError {
    ConnectorClientError {
        code: "workspace_file_failed".into(),
        message:
            "The working-folder file operation failed or the path is outside the working folder."
                .into(),
        resolution: None,
    }
}

fn invalid_response() -> ConnectorClientError {
    ConnectorClientError::local("invalid_connector_response")
}

/// Uploads one regular file from the CLI working folder through the host's
/// transfer owner: size and quota are accepted before any byte is read, and
/// bytes stream in bounded pieces without being held whole in memory.
///
/// # Errors
/// Fails when the path leaves the working folder, is not a regular file, or
/// the host refuses or drops the transfer.
pub async fn upload_path(
    client: &RoomConnectorClient,
    files: &ConnectorFileContext,
    path: &str,
    content_type: &str,
) -> Result<Value, ConnectorClientError> {
    if !client.attachment_transfers().await {
        return Err(ConnectorClientError {
            code: "workspace_upload_unavailable".into(),
            message: "This host does not advertise attachment transfers; use the inline upload."
                .into(),
            resolution: None,
        });
    }
    let workspace = files.workspace.clone();
    let identity = files.identity.clone();
    let requested = path.to_owned();
    // Capability resolution confines symlinks exactly as on managed runs; no byte is read here.
    let (file, filename, size) = tokio::task::spawn_blocking(move || {
        agentsassemble_provider::open_workspace_attachment(&workspace, &identity, &requested)
    })
    .await
    .map_err(|_| workspace_failed())?
    .map_err(|_| workspace_failed())?;
    let operation_id = uuid::Uuid::new_v4().to_string();
    let begin = client
        .upload_begin(&serde_json::json!({
            "filename": filename,
            "content_type": content_type,
            "declared_size": size,
            "operation_id": operation_id,
        }))
        .await?;
    let handle = transfer_string(&begin, "/transfer/handle")?;
    let mut offset = transfer_usize(&begin, "/transfer/accepted_offset")?;
    if offset > size {
        let _ = client.upload_cancel(&handle).await;
        return Err(invalid_response());
    }
    let mut file = tokio::fs::File::from_std(file);
    if offset > 0 {
        file.seek(std::io::SeekFrom::Start(offset as u64))
            .await
            .map_err(|_| workspace_failed())?;
    }
    let remaining = size - offset;
    if remaining > 0 {
        let body = tokio_util::io::ReaderStream::with_capacity(file, MESSAGE_UPLOAD_PIECE_BYTES);
        let status = client
            .upload_data(&handle, offset, reqwest::Body::wrap_stream(body))
            .await;
        let status = match status {
            Ok(status) => status,
            Err(error) => {
                let _ = client.upload_cancel(&handle).await;
                return Err(error);
            }
        };
        offset = transfer_usize(&status, "/transfer/accepted_offset")?;
        if offset != size {
            let _ = client.upload_cancel(&handle).await;
            return Err(invalid_response());
        }
    }
    let finished = client.upload_finish(&handle).await;
    let finished = match finished {
        Ok(finished) => finished,
        Err(error) => {
            let _ = client.upload_cancel(&handle).await;
            return Err(error);
        }
    };
    finished
        .pointer("/transfer/attachment")
        .cloned()
        .ok_or_else(invalid_response)
}

/// Streams one large published attachment into the CLI working-folder
/// subfolder; a truncated stream never publishes a partial file.
///
/// # Errors
/// Fails when the host refuses the download, the stream ends early, or the
/// file cannot be written into the working folder.
pub async fn download_to_workspace(
    client: &RoomConnectorClient,
    files: &ConnectorFileContext,
    attachment_id: &str,
) -> Result<agentsassemble_provider::ProviderAttachment, ConnectorClientError> {
    let (metadata, stream) = client.download_attachment(attachment_id).await?;
    let workspace = files.workspace.clone();
    let identity = files.identity.clone();
    let filename = metadata.filename.clone();
    let mut sink = tokio::task::spawn_blocking(move || {
        agentsassemble_provider::WorkspaceAttachmentSink::new(&workspace, &identity, &filename)
    })
    .await
    .map_err(|_| workspace_failed())?
    .map_err(|_| workspace_failed())?;
    tokio::pin!(stream);
    let mut received = 0usize;
    while let Some(next) = stream.next().await {
        let bytes = next?;
        received = received.saturating_add(bytes.len());
        if received > metadata.size {
            return Err(invalid_response());
        }
        sink.file
            .write_all(&bytes)
            .await
            .map_err(|_| workspace_failed())?;
    }
    if received != metadata.size {
        return Err(invalid_response());
    }
    let path = sink.publish().await.map_err(|_| workspace_failed())?;
    let mut attachment = crate::provider_attachment_runtime::into_provider_attachment(
        agentsassemble_persistence::MessageAttachment {
            metadata,
            content: Vec::new(),
        },
    );
    attachment.path = Some(path);
    Ok(attachment)
}

fn transfer_string(value: &Value, pointer: &str) -> Result<String, ConnectorClientError> {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(invalid_response)
}

fn transfer_usize(value: &Value, pointer: &str) -> Result<usize, ConnectorClientError> {
    value
        .pointer(pointer)
        .and_then(Value::as_u64)
        .and_then(|n| usize::try_from(n).ok())
        .ok_or_else(invalid_response)
}
