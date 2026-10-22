//! Retained response bytes share host admission across plain and virtual HTTP.
use agentsassemble_persistence::PersistenceError;
use axum::body::Bytes;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

// Whole-value assets/AI retain their round-4 costs. Streams reserve a fixed
// allowance for SQLx copies, a piece and Hyper's 256 KiB buffer with margin.
// 32 streams leave room for ten files, visible images, icons and other devices;
// their 32 MiB maximum shares the host envelope and principal weight with assets.
const STREAM_BYTES: usize = 1024 * 1024;
const PER_PRINCIPAL_STREAMS: usize = 32;
const HOST_BYTES: usize = 512 * 1024 * 1024;
// Human downloads reserve STREAM_BYTES even for a maximum-size room file.
const ENCODED_BYTES: usize = HOST_BYTES - 2 * STREAM_BYTES;
const COPIES: usize = 2;
const PER_PRINCIPAL: usize = 16;
const PER_PRINCIPAL_BYTES: usize = 400 * 1024 * 1024;
pub(crate) const ERROR_CODE: &str = "attachment_download_budget_exhausted";

/// Per principal: buffered downloads, bytes held, and open streams.
type PrincipalUsage = Arc<parking_lot::Mutex<HashMap<String, (usize, usize, usize)>>>;

#[derive(Clone)]
pub(crate) struct Budget {
    bytes: Arc<Semaphore>,
    encoded: Arc<Semaphore>,
    principals: PrincipalUsage,
}
impl Default for Budget {
    fn default() -> Self {
        Self {
            bytes: Arc::new(Semaphore::new(HOST_BYTES)),
            encoded: Arc::new(Semaphore::new(ENCODED_BYTES)),
            principals: Arc::default(),
        }
    }
}
pub(crate) struct Lease {
    _bytes: OwnedSemaphorePermit,
    _encoded: Option<OwnedSemaphorePermit>,
    principals: PrincipalUsage,
    principal: Option<String>,
    weight: usize,
    streamed: bool,
}
impl Drop for Lease {
    fn drop(&mut self) {
        let Some(principal) = &self.principal else {
            return;
        };
        let mut principals = self.principals.lock();
        // Every lease with a principal was counted when it was reserved.
        let Some(count) = principals.get_mut(principal) else {
            tracing::error!("a download lease outlived its principal's count");
            return;
        };
        if self.streamed {
            count.2 -= 1;
        } else {
            count.0 -= 1;
        }
        count.1 -= self.weight;
        if count.0 == 0 && count.2 == 0 {
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
    #[cfg(test)]
    pub(crate) fn available(&self) -> usize {
        self.bytes.available_permits()
    }
    pub(crate) fn reserve(
        &self,
        principal: Option<&str>,
        size: usize,
    ) -> Result<Lease, PersistenceError> {
        self.reserve_weight(
            principal,
            size.checked_mul(COPIES).ok_or_else(exhausted)?,
            false,
            None,
        )
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
        let encoded = self
            .encoded
            .clone()
            .try_acquire_many_owned(u32::try_from(weight).map_err(|_| exhausted())?)
            .map_err(|_| exhausted())?;
        self.reserve_weight(Some(principal), weight, false, Some(encoded))
    }

    pub(crate) fn reserve_stream(&self, principal: &str) -> Result<Lease, PersistenceError> {
        self.reserve_weight(Some(principal), STREAM_BYTES, true, None)
    }

    pub(crate) fn reserve_preview_generation(
        &self,
        principal: &str,
        size: usize,
    ) -> Result<Lease, PersistenceError> {
        // Whole encoded input/driver copies, bounded decoder/rotation buffers,
        // plus <=1100px resize and temporary encoding. Held by the blocking task.
        let decode = usize::try_from(agentsassemble_persistence::MAX_RASTER_DECODE_ALLOC_BYTES)
            .map_err(|_| exhausted())?;
        let weight = size
            .checked_mul(2)
            .and_then(|n| n.checked_add(2 * decode + 16 * 1024 * 1024))
            .ok_or_else(exhausted)?;
        self.reserve_weight(Some(principal), weight, false, None)
    }

    fn reserve_weight(
        &self,
        principal: Option<&str>,
        weight: usize,
        streamed: bool,
        encoded: Option<OwnedSemaphorePermit>,
    ) -> Result<Lease, PersistenceError> {
        let weight = u32::try_from(weight).map_err(|_| exhausted())?;
        let mut principals = self.principals.lock();
        if principal.is_some_and(|p| {
            let (count, used, streams) = principals.get(p).copied().unwrap_or_default();
            (if streamed {
                streams >= PER_PRINCIPAL_STREAMS
            } else {
                count >= PER_PRINCIPAL
            }) || used + weight as usize > PER_PRINCIPAL_BYTES
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
            if streamed {
                usage.2 += 1;
            } else {
                usage.0 += 1;
            }
            usage.1 += weight as usize;
        }
        Ok(Lease {
            _bytes: bytes,
            _encoded: encoded,
            principals: self.principals.clone(),
            principal: principal.map(str::to_owned),
            weight: weight as usize,
            streamed,
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
    #[test]
    fn streamed_fairness_and_host_weight_release_on_drop() {
        let budget = Budget::default();
        let mut streams = Vec::new();
        for principal in 0..16 {
            for _ in 0..32 {
                streams.push(budget.reserve_stream(&principal.to_string()).unwrap());
            }
            assert!(budget.reserve_stream(&principal.to_string()).is_err());
        }
        assert_eq!(budget.available(), 0);
        assert!(budget.reserve_stream("another").is_err());
        drop(streams);
        assert_eq!(budget.available(), HOST_BYTES);
        let streams = (0..10)
            .map(|_| budget.reserve_stream("one").unwrap())
            .collect::<Vec<_>>();
        let images = (0..16)
            .map(|_| budget.reserve(Some("one"), 65536).unwrap())
            .collect::<Vec<_>>();
        assert!(budget.reserve_encoded("one", 10 * 1024 * 1024).is_err());
        drop(images);
        assert!(budget.reserve_encoded("one", 10 * 1024 * 1024).is_ok());
        drop(streams);
        assert_eq!(budget.available(), HOST_BYTES);
    }
    #[test]
    fn full_encoded_class_leaves_two_maximum_human_streams() {
        let budget = Budget::default();
        let mut ai = (0..6)
            .map(|n| {
                budget
                    .reserve_encoded(
                        &format!("ai-{n}"),
                        agentsassemble_domain::MAX_ATTACHMENT_BYTES,
                    )
                    .unwrap()
            })
            .collect::<Vec<_>>();
        // Fill the residual class capacity with one smaller attachment.
        let residual = (budget.available() - 2 * STREAM_BYTES - 64 * 1024) / 8;
        ai.push(budget.reserve_encoded("ai-residual", residual).unwrap());
        assert!(budget.reserve_encoded("another-ai", 1).is_err());
        // The 50 MiB human file takes the same fixed stream weight as a small one.
        let human = [
            budget.reserve_stream("human-a").unwrap(),
            budget.reserve_stream("human-b").unwrap(),
        ];
        drop(human);
        drop(ai);
        assert!(
            budget
                .reserve_encoded("another-ai", agentsassemble_domain::MAX_ATTACHMENT_BYTES)
                .is_ok()
        );
        assert_eq!(budget.available(), HOST_BYTES);
    }
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
