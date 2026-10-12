//! One process-owned human transfer service behind both HTTP transports.
use agentsassemble_persistence::{
    MESSAGE_UPLOAD_PIECE_BYTES, MessageUploadAuthority, MessageUploadStatus, PersistenceError,
    SqliteStore,
};
use axum::{body::Body, http::StatusCode};
use futures_util::StreamExt;
use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::{http_body_policy::Policy, profile_web::ProfileHttpError};

const PER_HUMAN: usize = 3;
const HOST_TRANSFERS: usize = 64 * 1024 * 1024 / BUFFER_ALLOWANCE;
// Metadata/finished receipts are bounded separately and expire at begin + one hour.
const MAX_RECEIPTS: usize = 4096;
pub(crate) const BUFFER_ALLOWANCE: usize = 1024 * 1024;

#[derive(Clone)]
pub(crate) struct Service {
    store: SqliteStore,
    entries: Arc<Mutex<HashMap<String, Arc<Entry>>>>,
}
struct Entry {
    authority: MessageUploadAuthority,
    operation_id: String,
    filename: String,
    content_type: String,
    size: usize,
    expiry: i64,
    active: AtomicBool,
    cancel: CancellationToken,
    current: Mutex<EntryState>,
}
struct EntryState {
    status: MessageUploadStatus,
    cancelled: bool,
}

impl Service {
    pub(crate) fn new(store: SqliteStore) -> Self {
        Self {
            store,
            entries: Arc::default(),
        }
    }

    pub(crate) async fn cleanup_until_shutdown(&self, shutdown: CancellationToken) {
        let mut interval = tokio::time::interval(Duration::from_secs(10));
        loop {
            tokio::select! {
                ()=shutdown.cancelled() => { self.cleanup(true).await; return; },
                _=interval.tick() => self.cleanup(false).await,
            }
        }
    }
    async fn cleanup(&self, shutdown: bool) {
        let entries = self.entries.lock().await.clone();
        for (handle, entry) in entries {
            let expired = entry.expiry <= chrono::Utc::now().timestamp();
            let lost = if !expired && !shutdown && entry.active.load(Ordering::Relaxed) {
                match self
                    .store
                    .message_upload_status(&entry.authority, &handle)
                    .await
                {
                    Ok(_) => false,
                    Err(
                        PersistenceError::CommandRejected { .. }
                        | PersistenceError::RoomMissing
                        | PersistenceError::ParticipantMissing,
                    ) => true,
                    Err(_) => {
                        tracing::error!("Upload cleanup authority check failed");
                        continue;
                    }
                }
            } else {
                false
            };
            if expired || lost || shutdown || entry.cancel.is_cancelled() {
                entry.cancel.cancel();
                let mut current = entry.current.lock().await;
                if !expired && !shutdown && current.status.attachment.is_some() {
                    continue; // A failed/racing cancel cannot retire a committed receipt.
                }
                if self.store.discard_message_upload(&handle).await.is_err() {
                    tracing::error!("Upload cleanup storage failed");
                    continue;
                }
                current.cancelled = true;
                entry.active.store(false, Ordering::Relaxed);
                drop(current);
                self.entries.lock().await.remove(&handle);
            }
        }
    }

    pub(crate) async fn begin(
        &self,
        authority: MessageUploadAuthority,
        operation_id: String,
        filename: String,
        content_type: String,
        size: usize,
    ) -> Result<MessageUploadStatus, ProfileHttpError> {
        self.cleanup(false).await;
        // Admission and idempotent begin are serialized; no network is awaited here.
        let mut entries = self.entries.lock().await;
        let mut active = 0;
        let mut human = 0;
        for (handle, entry) in entries.iter() {
            if entry.authority.identity() == authority.identity()
                && entry.operation_id == operation_id
            {
                if !entry.authority.same_custody(&authority)
                    || entry.filename != filename
                    || entry.content_type != content_type
                    || entry.size != size
                {
                    return Err(error("upload_operation_conflict"));
                }
                return self
                    .store
                    .message_upload_status(&authority, handle)
                    .await
                    .map_err(Into::into);
            }
            if entry.active.load(Ordering::Relaxed) {
                active += 1;
                if entry.authority.identity().1 == authority.identity().1 {
                    human += 1;
                }
            }
        }
        if active >= HOST_TRANSFERS || human >= PER_HUMAN || entries.len() >= MAX_RECEIPTS {
            return Err(defer());
        }
        let status = self
            .store
            .begin_message_upload(&authority, &operation_id, &filename, &content_type, size)
            .await?;
        entries.insert(
            status.handle.clone(),
            Arc::new(Entry {
                authority,
                operation_id,
                filename,
                content_type,
                size,
                expiry: status.expires_at,
                active: AtomicBool::new(status.attachment.is_none()),
                cancel: CancellationToken::new(),
                current: Mutex::new(EntryState {
                    status: status.clone(),
                    cancelled: false,
                }),
            }),
        );
        Ok(status)
    }
    async fn entry(
        &self,
        authority: &MessageUploadAuthority,
        handle: &str,
    ) -> Result<Arc<Entry>, ProfileHttpError> {
        let entry = self
            .entries
            .lock()
            .await
            .get(handle)
            .cloned()
            .ok_or_else(|| error("upload_missing"))?;
        if !entry.authority.same_custody(authority) {
            return Err(error("upload_missing"));
        }
        Ok(entry)
    }

    pub(crate) async fn data(
        &self,
        authority: MessageUploadAuthority,
        handle: String,
        offset: usize,
        body: Body,
        policy: Policy,
    ) -> Result<MessageUploadStatus, ProfileHttpError> {
        let entry = self.entry(&authority, &handle).await?;
        let transfer = entry;
        let mut entry = transfer
            .current
            .try_lock()
            .map_err(|_| error("upload_request_busy"))?;
        if entry.cancelled {
            return Err(error("upload_missing"));
        }
        entry.status = self
            .store
            .message_upload_status(&authority, &handle)
            .await?;
        if entry.status.attachment.is_some() || entry.status.accepted_offset != offset {
            return Err(error("upload_offset_conflict"));
        }
        policy
            .authorize_upload()
            .map_err(ProfileHttpError::from_body)?;
        let mut stream = body.into_data_stream();
        while let Some(next) = tokio::select! { ()=transfer.cancel.cancelled()=>return Err(error("upload_cancelled")), next=stream.next()=>next }
        {
            let bytes = match next {
                Ok(bytes) => bytes,
                // An interrupted body is an explicit recoverable result, including its
                // durably accepted offset. Unaccepted input is never reported as received.
                Err(_) => {
                    return Err(ProfileHttpError::new(
                        StatusCode::REQUEST_TIMEOUT,
                        "upload_interrupted",
                        "Upload body interrupted; query status to resume.",
                    )
                    .with_offset(entry.status.accepted_offset));
                }
            };
            for piece in bytes.chunks(MESSAGE_UPLOAD_PIECE_BYTES) {
                let _server_wait = policy.server_wait();
                entry.status.accepted_offset = self
                    .store
                    .append_message_upload(&authority, &handle, entry.status.accepted_offset, piece)
                    .await?;
            }
        }
        Ok(entry.status.clone())
    }
    pub(crate) async fn status(
        &self,
        authority: MessageUploadAuthority,
        handle: String,
    ) -> Result<MessageUploadStatus, ProfileHttpError> {
        self.entry(&authority, &handle).await?;
        self.store
            .message_upload_status(&authority, &handle)
            .await
            .map_err(Into::into)
    }
    pub(crate) async fn finish(
        &self,
        authority: MessageUploadAuthority,
        handle: String,
    ) -> Result<MessageUploadStatus, ProfileHttpError> {
        let entry = self.entry(&authority, &handle).await?;
        let transfer = entry;
        let mut entry = transfer.current.lock().await;
        if entry.cancelled {
            return Err(error("upload_missing"));
        }
        entry.status = self
            .store
            .finish_message_upload(&authority, &handle)
            .await?;
        transfer.active.store(false, Ordering::Relaxed);
        Ok(entry.status.clone())
    }
    pub(crate) async fn cancel(
        &self,
        authority: MessageUploadAuthority,
        handle: String,
    ) -> Result<(), ProfileHttpError> {
        let entry = self.entry(&authority, &handle).await?;
        let status = self
            .store
            .message_upload_status(&authority, &handle)
            .await?;
        if status.attachment.is_some() {
            return Err(error("upload_already_finished"));
        }
        entry.cancel.cancel();
        let transfer = entry;
        let mut entry = transfer.current.lock().await;
        self.store
            .message_upload_status(&authority, &handle)
            .await?;
        if entry.status.attachment.is_some() {
            return Err(error("upload_already_finished"));
        }
        self.store.discard_message_upload(&handle).await?;
        entry.cancelled = true;
        transfer.active.store(false, Ordering::Relaxed);
        Ok(())
    }
}
fn error(code: &'static str) -> ProfileHttpError {
    ProfileHttpError::new(
        StatusCode::CONFLICT,
        code,
        "Upload transfer cannot accept this request.",
    )
}
fn defer() -> ProfileHttpError {
    ProfileHttpError::new(
        StatusCode::TOO_MANY_REQUESTS,
        "upload_deferred",
        "Upload capacity is busy; keep this file queued.",
    )
    .with_defer(1000)
}

#[cfg(test)]
mod tests;
