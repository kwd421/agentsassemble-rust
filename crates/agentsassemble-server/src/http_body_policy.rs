//! Router deadline starts before authority; only authorized file consumers extend it.
use crate::http_api::BodyDecodeError;
use axum::{body::Body, extract::Request, middleware::Next, response::Response};
use futures_util::StreamExt;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::time::{Instant, timeout_at};

const CONTROL: Duration = Duration::from_secs(10);
const IDLE: Duration = Duration::from_secs(30);
const WINDOW: Duration = Duration::from_secs(120);
const MIN_PROGRESS: usize = 64 * 1024;
pub(crate) const UPLOAD_CEILING: Duration = Duration::from_secs(60 * 60);

#[derive(Clone)]
pub(crate) struct Policy {
    started: Instant,
    upload: Arc<AtomicBool>,
}
impl Policy {
    pub(crate) fn new() -> Self {
        Self {
            started: Instant::now(),
            upload: Arc::new(AtomicBool::new(false)),
        }
    }
    pub(crate) fn authorize_upload(&self) -> Result<(), BodyDecodeError> {
        if Instant::now() >= self.started + CONTROL {
            return Err(BodyDecodeError::RequestTimeout);
        }
        self.upload.store(true, Ordering::Relaxed);
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
#[error("Request body progress deadline exceeded")]
pub(crate) struct UploadTimeout;

pub(crate) async fn apply(request: Request, next: Next) -> Response {
    next.run(wrap(request)).await
}

fn wrap(request: Request) -> Request {
    let (mut parts, body) = request.into_parts();
    let policy = Policy::new();
    let started = policy.started;
    parts.extensions.insert(policy.clone());
    let stream = futures_util::stream::try_unfold(
        (
            body.into_data_stream(),
            policy,
            started,
            started + WINDOW,
            0usize,
        ),
        |(mut body, policy, mut last_data, mut window_end, mut progress)| async move {
            loop {
                let now = Instant::now();
                let upload = policy.upload.load(Ordering::Relaxed);
                let hard_end = policy.started + if upload { UPLOAD_CEILING } else { CONTROL };
                if now >= hard_end || (upload && now >= last_data + IDLE) {
                    return Err(axum::Error::new(UploadTimeout));
                }
                if upload && now >= window_end {
                    if progress < MIN_PROGRESS {
                        return Err(axum::Error::new(UploadTimeout));
                    }
                    progress = 0;
                    window_end += WINDOW;
                    continue;
                }
                let deadline = if upload {
                    hard_end.min(last_data + IDLE).min(window_end)
                } else {
                    hard_end
                };
                match timeout_at(deadline, body.next()).await {
                    Err(_) => continue,
                    Ok(None) => return Ok(None),
                    Ok(Some(Err(error))) => return Err(error),
                    Ok(Some(Ok(bytes))) => {
                        if !bytes.is_empty() {
                            last_data = Instant::now();
                            progress = progress.saturating_add(bytes.len());
                        }
                        return Ok(Some((
                            bytes,
                            (body, policy, last_data, window_end, progress),
                        )));
                    }
                }
            }
        },
    );
    Request::from_parts(parts, Body::from_stream(stream))
}

#[cfg(test)]
mod tests;
