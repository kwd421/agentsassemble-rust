//! Retained response bytes share host admission across plain and virtual HTTP.
use agentsassemble_persistence::PersistenceError;
use axum::body::Bytes;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

// Two copies cover the SQLite result and response allocation while transferring.
// Five 50 MiB files fit concurrently, with room for small images. Four requests per
// principal match the shared frontend image scheduler without letting it own the host.
const HOST_BYTES: usize = 512 * 1024 * 1024;
const COPIES: usize = 2;
const PER_PRINCIPAL: usize = 4;
pub(crate) const ERROR_CODE: &str = "attachment_download_budget_exhausted";

#[derive(Clone)]
pub(crate) struct Budget {
    bytes: Arc<Semaphore>,
    principals: Arc<parking_lot::Mutex<HashMap<String, usize>>>,
}
impl Default for Budget {
    fn default() -> Self {
        Self {
            bytes: Arc::new(Semaphore::new(HOST_BYTES)),
            principals: Arc::default(),
        }
    }
}
pub(crate) struct Lease {
    _bytes: OwnedSemaphorePermit,
    principals: Arc<parking_lot::Mutex<HashMap<String, usize>>>,
    principal: Option<String>,
}
impl Drop for Lease {
    fn drop(&mut self) {
        let Some(principal) = &self.principal else {
            return;
        };
        let mut principals = self.principals.lock();
        let count = principals
            .get_mut(principal)
            .expect("live download principal");
        *count -= 1;
        if *count == 0 {
            principals.remove(principal);
        }
    }
}
fn exhausted() -> PersistenceError {
    PersistenceError::CommandRejected {
        code: ERROR_CODE.into(),
        message: "Attachment downloads are busy; retry after an active download finishes.".into(),
    }
}
impl Budget {
    pub(crate) fn reserve(
        &self,
        principal: Option<&str>,
        size: usize,
    ) -> Result<Lease, PersistenceError> {
        let weight = size
            .checked_mul(COPIES)
            .and_then(|n| u32::try_from(n).ok())
            .ok_or_else(exhausted)?;
        let mut principals = self.principals.lock();
        if principal.is_some_and(|p| principals.get(p).copied().unwrap_or(0) >= PER_PRINCIPAL) {
            return Err(exhausted());
        }
        let bytes = self
            .bytes
            .clone()
            .try_acquire_many_owned(weight)
            .map_err(|_| exhausted())?;
        if let Some(principal) = principal {
            *principals.entry(principal.to_owned()).or_default() += 1;
        }
        Ok(Lease {
            _bytes: bytes,
            principals: self.principals.clone(),
            principal: principal.map(str::to_owned),
        })
    }
}
impl Lease {
    pub(crate) fn retain(self, content: Vec<u8>) -> Bytes {
        Bytes::from_owner(ResponseBytes {
            content,
            _lease: self,
        })
    }
}
struct ResponseBytes {
    content: Vec<u8>,
    _lease: Lease,
}
impl AsRef<[u8]> for ResponseBytes {
    fn as_ref(&self) -> &[u8] {
        &self.content
    }
}
