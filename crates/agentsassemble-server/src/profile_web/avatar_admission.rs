use std::sync::Arc;

use axum::{body::Bytes, http::StatusCode, response::Response};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use super::{ProfileHttpError, attachment_response};

pub(super) fn acquire(
    semaphore: &Arc<Semaphore>,
) -> Result<OwnedSemaphorePermit, ProfileHttpError> {
    semaphore.clone().try_acquire_owned().map_err(|_| {
        ProfileHttpError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "too_many_requests",
            "잠시 후 다시 시도해 주세요.",
        )
    })
}

pub(super) fn response(
    filename: &str,
    content_type: &str,
    content: Vec<u8>,
    inline: bool,
    permit: OwnedSemaphorePermit,
) -> Result<Response, ProfileHttpError> {
    // Keep admission with the allocation, including transport-owned Bytes clones.
    let bytes = Bytes::from_owner(AvatarBytes {
        content,
        _permit: permit,
    });
    attachment_response(filename, content_type, bytes, inline)
}

struct AvatarBytes {
    content: Vec<u8>,
    _permit: OwnedSemaphorePermit,
}

impl AsRef<[u8]> for AvatarBytes {
    fn as_ref(&self) -> &[u8] {
        &self.content
    }
}
