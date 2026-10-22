//! Immutable validation diagnoses; CPU/storage/admission errors are never receipts.
use crate::PersistenceError;
use parking_lot::Mutex;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// A small immutable raster diagnosis, safe to retain without the input bytes.
#[derive(Clone, Debug)]
pub struct RasterValidationFailure {
    code: std::borrow::Cow<'static, str>,
    message: String,
}
impl RasterValidationFailure {
    /// Classify only content/format/dimension failures, never transient failures.
    #[must_use]
    pub fn from_error(error: &PersistenceError) -> Option<Self> {
        let PersistenceError::CommandRejected { code, message } = error else {
            return None;
        };
        matches!(
            code.as_ref(),
            "attachment_invalid_image"
                | "attachment_type_mismatch"
                | "attachment_image_limits"
                | "attachment_type_unsupported"
        )
        .then(|| Self {
            code: code.clone(),
            message: message.clone(),
        })
    }
    /// Reproduce the original validation error without re-assembling or decoding.
    #[must_use]
    pub fn error(&self) -> PersistenceError {
        PersistenceError::CommandRejected {
            code: self.code.clone(),
            message: self.message.clone(),
        }
    }
}

// No schema changes: derived failures live only for this store runtime. Capacity
// covers receipts AND work that may create one. Never evict a live failed original
// into repeated CPU work; reclaim deleted originals only when capacity is needed.
pub(crate) struct PreviewFailures {
    receipts: Mutex<HashMap<String, (RasterValidationFailure, OwnedSemaphorePermit)>>,
    slots: Arc<Semaphore>,
}
impl Default for PreviewFailures {
    fn default() -> Self {
        Self {
            receipts: Mutex::default(),
            slots: Arc::new(Semaphore::new(
                usize::try_from(crate::asset_storage::MAX_RETAINED_ASSETS).unwrap_or_default(),
            )),
        }
    }
}
impl PreviewFailures {
    pub(crate) fn get(&self, id: &str) -> Option<RasterValidationFailure> {
        self.receipts.lock().get(id).map(|(error, _)| error.clone())
    }
    pub(crate) async fn reserve(
        self: &Arc<Self>,
        id: &str,
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    ) -> Result<PreviewFailureSlot, PersistenceError> {
        let permit = if let Ok(permit) = self.slots.clone().try_acquire_owned() {
            permit
        } else {
            let live: HashSet<String> =
                sqlx::query_scalar("SELECT attachment_id FROM room_message_attachments")
                    .fetch_all(&mut **tx)
                    .await?
                    .into_iter()
                    .collect();
            self.receipts.lock().retain(|id, _| live.contains(id));
            self.slots.clone().try_acquire_owned().map_err(|_| {
                PersistenceError::CommandRejected {
                    code: "attachment_download_budget_exhausted".into(),
                    message: "Preview validation receipts are busy; retry later.".into(),
                }
            })?
        };
        Ok(PreviewFailureSlot {
            owner: self.clone(),
            id: id.to_owned(),
            permit,
        })
    }
}
pub(crate) struct PreviewFailureSlot {
    owner: Arc<PreviewFailures>,
    id: String,
    permit: OwnedSemaphorePermit,
}
impl PreviewFailureSlot {
    pub(crate) fn record(self, error: &PersistenceError) {
        if let Some(failure) = RasterValidationFailure::from_error(error) {
            self.owner
                .receipts
                .lock()
                .insert(self.id, (failure, self.permit));
        }
    }
}
