use std::{path::Path, time::Duration};

use agentsassemble_domain::{RuntimeRestartReceipt, RuntimeRestartStatus};
use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{UnixListener, UnixStream},
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::{
    AppState,
    runtime_control_socket::{socket_path, validate_endpoint},
    runtime_restart::RuntimeRestartError,
};

const MAX_FRAME: u16 = 4096;

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum RuntimeControlRequest {
    Status { operation_id: Option<Uuid> },
    Restart { operation_id: Uuid },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum RuntimeControlResponse {
    Snapshot { runtime: RuntimeRestartStatus },
    Accepted { operation: RuntimeRestartReceipt },
    Error { code: String, message: String },
}

/// Serves one request per connection and finishes accepted responses before cancellation.
///
/// # Errors
/// Returns listener failures; individual client transport failures end only that connection.
pub async fn serve(
    listener: UnixListener,
    state: AppState,
    cancellation: CancellationToken,
) -> anyhow::Result<()> {
    loop {
        let (stream, _) = tokio::select! {
            biased;
            () = cancellation.cancelled() => return Ok(()),
            accepted = listener.accept() => accepted?,
        };
        if stream.peer_cred()?.uid() != rustix::process::geteuid().as_raw() {
            continue;
        }
        if let Err(error) = handle(stream, &state).await {
            tracing::debug!(%error, "runtime control client connection ended");
        }
    }
}

async fn handle(mut stream: UnixStream, state: &AppState) -> anyhow::Result<()> {
    let response = match read_frame(&mut stream).await {
        Ok(bytes) => match serde_json::from_slice::<RuntimeControlRequest>(&bytes) {
            Ok(RuntimeControlRequest::Status { operation_id }) => {
                match state
                    .runtime_restart
                    .status(&state.store, operation_id)
                    .await
                {
                    Ok(runtime) => RuntimeControlResponse::Snapshot { runtime },
                    Err(_) => error_response(
                        "runtime_restart_state",
                        "Runtime restart status is unavailable.",
                    ),
                }
            }
            Ok(RuntimeControlRequest::Restart { operation_id }) => {
                match state.runtime_restart.request(state, operation_id).await {
                    Ok(operation) => RuntimeControlResponse::Accepted { operation },
                    Err(error) => restart_error(&error),
                }
            }
            Err(_) => error_response("invalid_request", "Runtime control request is invalid."),
        },
        Err(_) => error_response(
            "invalid_request",
            "Runtime control frame is invalid or incomplete.",
        ),
    };
    stream.write_all(&serde_json::to_vec(&response)?).await?;
    stream.shutdown().await?;
    Ok(())
}

/// Sends a same-user private request. A missing response remains a transport error.
///
/// # Errors
/// Rejects insecure endpoints, different peer users, malformed replies and I/O failures.
pub async fn request(
    database: &Path,
    command: RuntimeControlRequest,
) -> anyhow::Result<RuntimeControlResponse> {
    let path = socket_path(database)?;
    validate_endpoint(&path)?;
    let mut stream =
        tokio::time::timeout(Duration::from_secs(5), UnixStream::connect(path)).await??;
    if stream.peer_cred()?.uid() != rustix::process::geteuid().as_raw() {
        anyhow::bail!("runtime control peer has a different OS owner");
    }
    stream.write_all(&serde_json::to_vec(&command)?).await?;
    stream.shutdown().await?;
    let bytes = read_frame(&mut stream).await?;
    Ok(serde_json::from_slice(&bytes)?)
}

async fn read_frame(stream: &mut UnixStream) -> anyhow::Result<Vec<u8>> {
    // The request's EOF is its frame boundary; this avoids any next-frame read-ahead.
    let mut bytes = Vec::new();
    tokio::time::timeout(
        Duration::from_secs(20),
        stream
            .take(u64::from(MAX_FRAME) + 1)
            .read_to_end(&mut bytes),
    )
    .await??;
    if bytes.is_empty() || bytes.len() > usize::from(MAX_FRAME) {
        anyhow::bail!("runtime control frame has an invalid size");
    }
    Ok(bytes)
}

fn restart_error(error: &RuntimeRestartError) -> RuntimeControlResponse {
    let code = match error {
        RuntimeRestartError::Unavailable => "runtime_restart_unavailable",
        RuntimeRestartError::Busy => "runtime_restart_busy",
        RuntimeRestartError::Candidate => "runtime_restart_candidate",
        RuntimeRestartError::Persistence(
            agentsassemble_persistence::PersistenceError::CommandRejected { code, .. },
        ) => code.as_ref(),
        RuntimeRestartError::Persistence(_) => "runtime_restart_state",
    };
    error_response(code, &error.to_string())
}

fn error_response(code: &str, message: &str) -> RuntimeControlResponse {
    RuntimeControlResponse::Error {
        code: code.to_owned(),
        message: message.to_owned(),
    }
}
