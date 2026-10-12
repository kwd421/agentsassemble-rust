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
pub(crate) const IDLE: Duration = Duration::from_secs(30);
const WINDOW: Duration = Duration::from_secs(120);
const MIN_PROGRESS: usize = 64 * 1024;
pub(crate) const UPLOAD_CEILING: Duration = Duration::from_secs(60 * 60);

#[derive(Clone)]
pub(crate) struct Policy {
    started: Instant,
    upload: Arc<AtomicBool>,
    clock: tokio::sync::watch::Sender<ClientClock>,
}
#[derive(Clone, Copy, Default)]
struct ClientClock {
    excluded: Duration,
    paused: Option<Instant>,
    owners: usize,
}
pub(crate) struct ServerWait(Policy);
impl Drop for ServerWait {
    fn drop(&mut self) {
        self.0.clock.send_modify(|clock| {
            clock.owners -= 1;
            if clock.owners == 0 {
                if let Some(start) = clock.paused.take() {
                    clock.excluded += start.elapsed();
                }
            }
        });
    }
}
impl Policy {
    pub(crate) fn new() -> Self {
        Self {
            started: Instant::now(),
            upload: Arc::new(AtomicBool::new(false)),
            clock: tokio::sync::watch::channel(ClientClock::default()).0,
        }
    }
    pub(crate) fn authorize_upload(&self) -> Result<(), BodyDecodeError> {
        if Instant::now() >= self.started + CONTROL {
            return Err(BodyDecodeError::RequestTimeout);
        }
        self.upload.store(true, Ordering::Relaxed);
        Ok(())
    }
    /// Storage/credit waits don't consume the client's idle or progress allowance.
    pub(crate) fn server_wait(&self) -> ServerWait {
        self.clock.send_modify(|clock| {
            if clock.owners == 0 {
                clock.paused = Some(Instant::now());
            }
            clock.owners += 1;
        });
        ServerWait(self.clone())
    }
    fn client_now(&self) -> Instant {
        let clock = self.clock.borrow();
        Instant::now()
            - clock.excluded
            - clock.paused.map_or(Duration::ZERO, |start| start.elapsed())
    }
}

#[derive(Debug, thiserror::Error)]
#[error("Request body progress deadline exceeded")]
pub(crate) struct UploadTimeout;

/// The same idle, minimum-progress window and ceiling govern outgoing file bodies.
#[derive(Clone)]
pub(crate) struct Progress(tokio::sync::watch::Sender<ProgressWindow>);

#[derive(Clone, Copy)]
struct ProgressWindow {
    started: Instant,
    last_data: Instant,
    window_end: Instant,
    bytes: usize,
}
impl ProgressWindow {
    fn new(started: Instant) -> Self {
        Self {
            started,
            last_data: started,
            window_end: started + WINDOW,
            bytes: 0,
        }
    }
    fn deadline(&mut self) -> Result<Instant, UploadTimeout> {
        self.deadline_at(Instant::now())
    }
    fn deadline_at(&mut self, now: Instant) -> Result<Instant, UploadTimeout> {
        if now >= self.started + UPLOAD_CEILING || now >= self.last_data + IDLE {
            return Err(UploadTimeout);
        }
        while now >= self.window_end {
            if self.bytes < MIN_PROGRESS {
                return Err(UploadTimeout);
            }
            self.bytes = 0;
            self.window_end += WINDOW;
        }
        Ok((self.started + UPLOAD_CEILING)
            .min(self.last_data + IDLE)
            .min(self.window_end))
    }
    fn accept(&mut self, bytes: usize) {
        self.accept_at(bytes, Instant::now());
    }
    fn accept_at(&mut self, bytes: usize, now: Instant) {
        if bytes > 0 {
            self.last_data = now;
            self.bytes = self.bytes.saturating_add(bytes);
        }
    }
}
impl Progress {
    pub(crate) fn new() -> Self {
        Self(tokio::sync::watch::channel(ProgressWindow::new(Instant::now())).0)
    }
    pub(crate) fn accept(&self, bytes: usize) {
        self.0.send_modify(|window| window.accept(bytes));
    }
    pub(crate) fn check(&self) -> Result<(), UploadTimeout> {
        let mut result = Ok(());
        self.0.send_if_modified(|window| {
            result = window.deadline().map(|_| ());
            false
        });
        result
    }
    pub(crate) async fn expired(&self) {
        let mut changes = self.0.subscribe();
        loop {
            changes.borrow_and_update();
            let mut result = Err(UploadTimeout);
            self.0.send_if_modified(|window| {
                result = window.deadline();
                false
            });
            let Ok(deadline) = result else {
                return;
            };
            tokio::select! {
                () = tokio::time::sleep_until(deadline) => {},
                _ = changes.changed() => {},
            }
        }
    }
}

pub(crate) async fn apply(request: Request, next: Next) -> Response {
    next.run(wrap(request)).await
}

pub(crate) fn wrap(request: Request) -> Request {
    let (mut parts, body) = request.into_parts();
    let policy = parts
        .extensions
        .get::<Policy>()
        .cloned()
        .unwrap_or_else(Policy::new);
    let started = policy.started;
    parts.extensions.insert(policy.clone());
    let stream = futures_util::stream::try_unfold(
        (
            body.into_data_stream(),
            policy,
            ProgressWindow::new(started),
        ),
        |(mut body, policy, mut progress)| async move {
            let mut changes = policy.clock.subscribe();
            let received = {
                let next = body.next();
                tokio::pin!(next);
                loop {
                    let now = Instant::now();
                    let upload = policy.upload.load(Ordering::Relaxed);
                    let hard_end = policy.started + if upload { UPLOAD_CEILING } else { CONTROL };
                    if now >= hard_end {
                        return Err(axum::Error::new(UploadTimeout));
                    }
                    let deadline = if upload && policy.clock.borrow().paused.is_none() {
                        let client_now = policy.client_now();
                        let deadline =
                            progress.deadline_at(client_now).map_err(axum::Error::new)?;
                        (now + deadline.saturating_duration_since(client_now)).min(hard_end)
                    } else {
                        hard_end
                    };
                    let received = tokio::select! {
                        _ = changes.changed() => continue,
                        received = timeout_at(deadline, &mut next) => received,
                    };
                    if let Ok(received) = received {
                        break received;
                    }
                }
            };
            match received {
                None => Ok(None),
                Some(Err(error)) => Err(error),
                Some(Ok(bytes)) => {
                    progress.accept_at(bytes.len(), policy.client_now());
                    Ok(Some((bytes, (body, policy, progress))))
                }
            }
        },
    );
    Request::from_parts(parts, Body::from_stream(stream))
}

#[cfg(test)]
mod tests;
