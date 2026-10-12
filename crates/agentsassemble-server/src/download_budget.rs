//! Retained response bytes share host admission across plain and virtual HTTP.
use agentsassemble_persistence::PersistenceError;
use axum::body::Bytes;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

// Two copies cover the SQLite result and response allocation while transferring.
// Five 50 MiB files fit concurrently, with room for small images. One client's image
// scheduler runs four reads, and room icons and the same person's other devices read
// outside it; a refusal has no client retry, so the count leaves room for all of them.
// Host bytes, not this count, bound memory.
const HOST_BYTES: usize = 512 * 1024 * 1024;
const COPIES: usize = 2;
const PER_PRINCIPAL: usize = 16;
const PER_PRINCIPAL_BYTES: usize = 400 * 1024 * 1024;
pub(crate) const ERROR_CODE: &str = "attachment_download_budget_exhausted";

#[derive(Clone)]
pub(crate) struct Budget {
    bytes: Arc<Semaphore>,
    principals: Arc<parking_lot::Mutex<HashMap<String, (usize, usize)>>>,
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
    principals: Arc<parking_lot::Mutex<HashMap<String, (usize, usize)>>>,
    principal: Option<String>,
    weight: usize,
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
        count.0 -= 1;
        count.1 -= self.weight;
        if count.0 == 0 {
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
        self.reserve_weight(principal, size.checked_mul(COPIES).ok_or_else(exhausted)?)
    }

    pub(crate) fn reserve_encoded(
        &self,
        principal: &str,
        size: usize,
    ) -> Result<Lease, PersistenceError> {
        // Raw bytes, base64, serialization capacity, and copied pipe frame capacity.
        let weight = size
            .checked_mul(8)
            .and_then(|n| n.checked_add(64 * 1024))
            .ok_or_else(exhausted)?;
        self.reserve_weight(Some(principal), weight)
    }

    fn reserve_weight(
        &self,
        principal: Option<&str>,
        weight: usize,
    ) -> Result<Lease, PersistenceError> {
        let weight = u32::try_from(weight).map_err(|_| exhausted())?;
        let mut principals = self.principals.lock();
        if principal.is_some_and(|p| {
            let (count, used) = principals.get(p).copied().unwrap_or_default();
            count >= PER_PRINCIPAL || used + weight as usize > PER_PRINCIPAL_BYTES
        }) {
            return Err(exhausted());
        }
        let bytes = self
            .bytes
            .clone()
            .try_acquire_many_owned(weight)
            .map_err(|_| exhausted())?;
        if let Some(principal) = principal {
            let usage = principals.entry(principal.to_owned()).or_default();
            usage.0 += 1;
            usage.1 += weight as usize;
        }
        Ok(Lease {
            _bytes: bytes,
            principals: self.principals.clone(),
            principal: principal.map(str::to_owned),
            weight: weight as usize,
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

pub(crate) fn encoded_response(
    value: &impl serde::Serialize,
    lease: Option<Lease>,
) -> Result<axum::response::Response, serde_json::Error> {
    use axum::response::IntoResponse;
    let content = serde_json::to_vec(value)?;
    let bytes = match lease {
        Some(lease) => lease.retain(content),
        None => content.into(),
    };
    Ok((
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        bytes,
    )
        .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn encoded_downloads_retain_shared_weight_until_delivery_bytes_drop() {
        let budget = Budget::default();
        let blocking = budget.reserve(None, 200 * 1024 * 1024).unwrap();
        // Provider raw bytes carry admission to the frame owner; HTTP retains encoded bytes.
        for kind in ["provider", "attendee", "connector"] {
            let lease = budget.reserve_encoded(kind, 10 * 1024 * 1024).unwrap();
            let retained = if kind == "provider" {
                lease.retain(vec![1])
            } else {
                let response =
                    encoded_response(&serde_json::json!({"content_base64":"AQ=="}), Some(lease))
                        .unwrap();
                axum::body::to_bytes(response.into_body(), 1024)
                    .await
                    .unwrap()
            };
            assert!(budget.reserve_encoded("another", 10 * 1024 * 1024).is_err());
            let clone = retained.clone();
            drop(retained);
            assert!(budget.reserve_encoded("another", 10 * 1024 * 1024).is_err());
            drop(clone);
            assert!(budget.reserve_encoded("another", 10 * 1024 * 1024).is_ok());
        }
        drop(blocking);
    }
}
