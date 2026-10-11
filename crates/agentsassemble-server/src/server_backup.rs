//! Runtime-owned export lifecycle. Native supplies only a save-dialog destination.
use crate::{
    AppState,
    http_api::{consume_local_operator, decode_json_body, exact_tauri_cors},
};
use agentsassemble_persistence::{
    ServerOwnerAuthority,
    backup_codec::{self, BackupManifest, KdfParameters},
    backup_database,
};
use anyhow::{Context, ensure};
use axum::{
    Json, Router,
    extract::{Request, State},
    http::{Method, StatusCode},
    response::{IntoResponse, Response},
};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Write},
    path::PathBuf,
    sync::Arc,
};
use tokio_util::sync::CancellationToken;
use zeroize::Zeroizing;

#[path = "server_backup_failure.rs"]
mod failure;
#[path = "server_backup_journal.rs"]
mod journal;
use failure::FailureReason;

#[derive(Clone, Default)]
pub(crate) struct BackupControl(Arc<Mutex<Option<Job>>>);
struct Job {
    registration_epoch: Option<String>,
    progress: Progress,
    cancel: CancellationToken,
    record: journal::Record,
    root: PathBuf,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Progress {
    operation_id: uuid::Uuid,
    phase: String,
    snapshot_at: Option<chrono::DateTime<chrono::Utc>>,
    bytes: Option<u64>,
    #[serde(default)]
    failure_reason: Option<FailureReason>,
}
#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
enum Control {
    Start {
        destination: PathBuf,
        passphrase: Option<String>,
        owner_person_id: String,
        server_name: String,
    },
    Latest,
    Status {
        operation_id: uuid::Uuid,
    },
    Cancel {
        operation_id: uuid::Uuid,
    },
}
registered_routes! {
    fn backup_routes<AppState>() {
        private "/api/server-backup" => post(control),
    }
}
pub(crate) fn routes() -> Router<AppState> {
    backup_routes().layer(exact_tauri_cors([Method::POST]))
}
struct Failure(StatusCode);
impl IntoResponse for Failure {
    fn into_response(self) -> Response {
        tracing::error!(status = self.0.as_u16(), "server backup control rejected");
        (self.0,Json(serde_json::json!({"error":{"code":"server_backup_failed","message":"Server backup failed; existing files were not replaced."}}))).into_response()
    }
}
async fn control(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<Option<Progress>>, Failure> {
    consume_local_operator(&state, request.headers())
        .await
        .ok_or(Failure(StatusCode::FORBIDDEN))?;
    let input: Control = decode_json_body(request, 16384)
        .await
        .map_err(|_| Failure(StatusCode::BAD_REQUEST))?;
    let epoch = state
        .store
        .registration_epoch()
        .await
        .map_err(|_| Failure(StatusCode::CONFLICT))?;
    let restricted = state
        .store
        .hosting_restriction()
        .await
        .map_err(|_| Failure(StatusCode::CONFLICT))?
        .is_some();
    match input {
        Control::Start {
            destination,
            passphrase,
            owner_person_id,
            server_name,
        } => start(
            &state,
            destination,
            passphrase,
            owner_person_id,
            server_name,
        )
        .await
        .map(|Json(progress)| Json(Some(progress))),
        Control::Latest => Ok(Json(
            state
                .server_backup
                .0
                .lock()
                .as_ref()
                .filter(|job| !restricted && epoch.is_some() && job.registration_epoch == epoch)
                .map(|job| job.progress.clone()),
        )),
        Control::Status { operation_id } | Control::Cancel { operation_id } => {
            let mut current = state.server_backup.0.lock();
            let job = current
                .as_mut()
                .filter(|j| {
                    !restricted
                        && epoch.is_some()
                        && j.registration_epoch == epoch
                        && j.progress.operation_id == operation_id
                })
                .ok_or(Failure(StatusCode::NOT_FOUND))?;
            if matches!(input, Control::Cancel { .. })
                && !matches!(job.progress.phase.as_str(), "complete" | "publishing")
            {
                job.cancel.cancel();
            }
            Ok(Json(Some(job.progress.clone())))
        }
    }
}
async fn start(
    state: &AppState,
    destination: PathBuf,
    passphrase: Option<String>,
    owner_person_id: String,
    server_name: String,
) -> Result<Json<Progress>, Failure> {
    if !destination.is_absolute()
        || destination.file_name().is_none()
        || server_name.len() > 400
        || owner_person_id.len() > 256
        || passphrase
            .as_ref()
            .is_some_and(|p| p.is_empty() || p.len() > 4096)
    {
        return Err(Failure(StatusCode::BAD_REQUEST));
    }
    let epoch = state
        .store
        .registration_epoch()
        .await
        .map_err(|_| Failure(StatusCode::CONFLICT))?
        .ok_or(Failure(StatusCode::CONFLICT))?;
    state
        .central_directory
        .current_host_owner(
            crate::central::directory::RedeemHost {
                identity: &state.central_host_identity,
                store: &state.store,
            },
            &owner_person_id,
            &epoch,
        )
        .await
        .map_err(|_| Failure(StatusCode::FORBIDDEN))?;
    let progress = Progress {
        operation_id: uuid::Uuid::new_v4(),
        phase: "snapshot".into(),
        snapshot_at: None,
        bytes: None,
        failure_reason: None,
    };
    let cancel = state.shutdown.child_token();
    {
        let mut current = state.server_backup.0.lock();
        if current.as_ref().is_some_and(|j| {
            !matches!(
                j.progress.phase.as_str(),
                "complete" | "failed" | "cancelled" | "interrupted"
            )
        }) {
            return Err(Failure(StatusCode::CONFLICT));
        }
        let root = state
            .runtime_state_root
            .clone()
            .ok_or(Failure(StatusCode::CONFLICT))?;
        let job = Job {
            registration_epoch: Some(epoch.clone()),
            progress: progress.clone(),
            cancel: cancel.clone(),
            record: journal::Record::default(),
            root,
        };
        journal::save(&job).map_err(|_| Failure(StatusCode::INTERNAL_SERVER_ERROR))?;
        *current = Some(job);
    }
    let next = state.clone();
    let id = progress.operation_id;
    state.connections.spawn(async move {
        let result = export(
            &next,
            id,
            ExportInput {
                destination,
                passphrase: passphrase.map(Zeroizing::new),
                owner: owner_person_id,
                name: server_name,
                epoch,
            },
            &cancel,
        )
        .await;
        if let Err(error) = result {
            next.server_backup.report_failure(id, &error);
            if next
                .server_backup
                .phase(
                    id,
                    if cancel.is_cancelled() {
                        "cancelled"
                    } else {
                        "failed"
                    },
                )
                .is_err()
            {
                tracing::error!("backup failure status could not be persisted");
            }
        }
    });
    Ok(Json(progress))
}

impl BackupControl {
    fn phase(&self, id: uuid::Uuid, phase: &'static str) -> anyhow::Result<()> {
        if let Some(job) = self
            .0
            .lock()
            .as_mut()
            .filter(|job| job.progress.operation_id == id)
        {
            job.progress.phase = if matches!(phase, "failed" | "cancelled")
                && matches!(job.progress.phase.as_str(), "publishing" | "complete")
            {
                "interrupted"
            } else {
                phase
            }
            .into();
            journal::save(job)?;
        }
        Ok(())
    }
}
struct Cancellable<R> {
    inner: R,
    cancel: CancellationToken,
}
impl<R: Read> Read for Cancellable<R> {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        if self.cancel.is_cancelled() {
            return Err(std::io::Error::from(std::io::ErrorKind::Other));
        }
        self.inner.read(bytes)
    }
}
struct ExportInput {
    destination: PathBuf,
    passphrase: Option<Zeroizing<String>>,
    owner: String,
    name: String,
    epoch: String,
}
async fn export(
    state: &AppState,
    id: uuid::Uuid,
    input: ExportInput,
    cancel: &CancellationToken,
) -> anyhow::Result<()> {
    let ExportInput {
        destination,
        passphrase,
        owner,
        name,
        epoch,
    } = input;
    let root = state
        .runtime_state_root
        .as_ref()
        .context("runtime state directory missing")?;
    let staging = tempfile::Builder::new()
        .prefix(&format!("server-backup-{id}-"))
        .tempdir_in(root)?;
    agentsassemble_persistence::secure_private_directory(staging.path())?;
    state.server_backup.record_path(id, staging.path(), false)?;
    let payload = staging.path().join("payload.sqlite");
    let snapshot_at = state
        .store
        .export_backup_payload(
            &ServerOwnerAuthority::LocalOperator,
            &staging.path().join("snapshot.sqlite"),
            &payload,
        )
        .await?;
    ensure!(!cancel.is_cancelled(), "cancelled");
    let (rooms, messages, assets) = backup_database::payload_counts(&payload).await?;
    let (database_bytes, database_sha256) =
        backup_codec::measure_payload(&mut std::fs::File::open(&payload)?)?;
    let manifest = BackupManifest {
        schema_version: agentsassemble_persistence::CURRENT_SCHEMA_VERSION,
        server_id: state.central_host_identity.server_id().into(),
        server_name: name,
        issuer: state.central_directory.issuer()?,
        owner_person_id: owner,
        source_epoch: epoch,
        source_key_fingerprint: state.central_host_identity.fingerprint().into(),
        snapshot_at,
        rooms,
        messages,
        assets,
        database_bytes,
        database_sha256,
    };
    let control = state.server_backup.clone();
    let cancel = cancel.clone();
    tokio::task::spawn_blocking(move || {
        finish_export(
            id,
            ExportFile {
                destination,
                passphrase,
                payload,
                manifest,
                staging,
            },
            &control,
            &cancel,
        )
    })
    .await??;
    Ok(())
}
struct ExportFile {
    destination: PathBuf,
    passphrase: Option<Zeroizing<String>>,
    payload: PathBuf,
    manifest: BackupManifest,
    staging: tempfile::TempDir,
}
fn finish_export(
    id: uuid::Uuid,
    file: ExportFile,
    control: &BackupControl,
    cancel: &CancellationToken,
) -> anyhow::Result<()> {
    let ExportFile {
        destination,
        passphrase,
        payload,
        manifest,
        staging,
    } = file;
    let snapshot_at = manifest.snapshot_at;
    let database_bytes = manifest.database_bytes;
    let parent = destination.parent().context("save directory missing")?;
    if fs2::available_space(parent)?
        <= database_bytes
            .saturating_add(database_bytes / 100)
            .saturating_add(8192)
    {
        return Err(std::io::Error::from(std::io::ErrorKind::StorageFull).into());
    }
    let mut partial = tempfile::Builder::new()
        .prefix(&format!(".aa-backup-{id}-"))
        .suffix(".partial")
        .tempfile_in(parent)?;
    control.record_path(id, partial.path(), true)?;
    control.phase(id, "encoding")?;
    let mut input = Cancellable {
        inner: std::fs::File::open(&payload)?,
        cancel: cancel.clone(),
    };
    backup_codec::encode(
        &mut input,
        partial.as_file_mut(),
        &manifest,
        passphrase.as_ref().map(|s| s.as_bytes()),
        KdfParameters::default(),
    )?;
    partial.flush()?;
    partial.as_file().sync_all()?;
    control.phase(id, "verifying")?;
    let mut input = Cancellable {
        inner: std::fs::File::open(partial.path())?,
        cancel: cancel.clone(),
    };
    let decoded = backup_codec::decode(
        &mut input,
        &mut std::io::sink(),
        passphrase.as_ref().map(|s| s.as_bytes()),
    )?;
    ensure!(
        decoded.database_sha256 == manifest.database_sha256,
        "backup verification changed"
    );
    // Cancellation and publication have one mutex-ordered linearization point.
    let mut current = control.0.lock();
    let job = current
        .as_mut()
        .filter(|j| j.progress.operation_id == id)
        .context("job changed")?;
    ensure!(!cancel.is_cancelled(), "cancelled");
    job.progress.phase = "publishing".into();
    journal::save(job)?;
    let bytes = partial.as_file().metadata()?.len();
    partial.persist_noclobber(&destination)?;
    #[cfg(unix)]
    std::fs::File::open(parent)?.sync_all()?;
    job.progress = Progress {
        operation_id: id,
        phase: "complete".into(),
        snapshot_at: Some(snapshot_at),
        bytes: Some(bytes),
        failure_reason: None,
    };
    journal::save(job)?;
    drop(staging);
    Ok::<_, anyhow::Error>(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn export_verifies_bytes_preserves_existing_file_and_honors_cancellation() -> anyhow::Result<()>
    {
        let directory = tempfile::tempdir()?;
        for scenario in ["success", "existing", "cancelled"] {
            let staging = tempfile::tempdir()?;
            let payload = staging.path().join("payload.sqlite");
            let bytes = vec![71_u8; 4096];
            std::fs::write(&payload, &bytes)?;
            let (database_bytes, database_sha256) =
                backup_codec::measure_payload(&mut std::fs::File::open(&payload)?)?;
            let manifest = BackupManifest {
                schema_version: agentsassemble_persistence::CURRENT_SCHEMA_VERSION,
                server_id: uuid::Uuid::new_v4().to_string(),
                server_name: "Owner server".into(),
                issuer: "https://central.example".into(),
                owner_person_id: "owner-person".into(),
                source_epoch: "epoch".into(),
                source_key_fingerprint: "f".repeat(43),
                snapshot_at: chrono::Utc::now(),
                rooms: 0,
                messages: 0,
                assets: 0,
                database_bytes,
                database_sha256,
            };
            let destination = directory.path().join(format!("{scenario}.aaserver"));
            if scenario == "existing" {
                std::fs::write(&destination, b"user-owned file")?;
            }
            let id = uuid::Uuid::new_v4();
            let cancel = CancellationToken::new();
            if scenario == "cancelled" {
                cancel.cancel();
            }
            let control = BackupControl::default();
            *control.0.lock() = Some(Job {
                registration_epoch: Some("test-epoch".into()),
                progress: Progress {
                    operation_id: id,
                    phase: "snapshot".into(),
                    snapshot_at: None,
                    bytes: None,
                    failure_reason: None,
                },
                cancel: cancel.clone(),
                record: journal::Record::default(),
                root: directory.path().to_owned(),
            });
            let result = finish_export(
                id,
                ExportFile {
                    destination: destination.clone(),
                    passphrase: None,
                    payload,
                    manifest,
                    staging,
                },
                &control,
                &cancel,
            );
            if scenario == "success" {
                result?;
                let mut decoded = Vec::new();
                backup_codec::decode(&mut std::fs::File::open(&destination)?, &mut decoded, None)?;
                assert_eq!(decoded, bytes);
                assert_eq!(
                    control
                        .0
                        .lock()
                        .as_ref()
                        .context("missing progress")?
                        .progress
                        .phase,
                    "complete"
                );
            } else {
                assert!(result.is_err());
                if scenario == "existing" {
                    assert_eq!(std::fs::read(destination)?, b"user-owned file");
                } else {
                    assert!(!destination.exists());
                }
            }
        }
        assert!(std::fs::read_dir(directory.path())?.all(|entry| {
            entry.is_ok_and(|entry| !entry.file_name().to_string_lossy().ends_with(".partial"))
        }));
        Ok(())
    }
}
