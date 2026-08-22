use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use axum::Router;
use hyper::server::conn::http1;
use hyper_util::{
    rt::{TokioIo, TokioTimer},
    service::TowerToHyperService,
};
use tokio::{net::TcpStream, sync::OwnedSemaphorePermit};

pub(crate) const MAX_HTTP_CONNECTIONS: usize = 128;
pub(crate) const HTTP_CONNECTION_LIFETIME: Duration = Duration::from_secs(30);
const HTTP_HEADER_TIMEOUT: Duration = Duration::from_secs(3);
const MAX_HTTP_BUFFER_BYTES: usize = 256 * 1024;

#[derive(Clone, Default)]
pub(crate) struct RejectionCounter(Arc<AtomicU64>);

impl RejectionCounter {
    pub(crate) fn record(&self) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn total(&self) -> u64 {
        self.0.load(Ordering::Relaxed)
    }
}

pub(crate) async fn serve_connection(
    stream: TcpStream,
    app: Router,
    _permit: OwnedSemaphorePermit,
) {
    let mut builder = http1::Builder::new();
    builder
        .timer(TokioTimer::new())
        .header_read_timeout(HTTP_HEADER_TIMEOUT)
        .max_buf_size(MAX_HTTP_BUFFER_BYTES);
    let connection = builder
        .serve_connection(TokioIo::new(stream), TowerToHyperService::new(app))
        .with_upgrades();
    match tokio::time::timeout(HTTP_CONNECTION_LIFETIME, connection).await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => tracing::debug!(error = ?error, "HTTP connection closed"),
        Err(_) => tracing::debug!("HTTP connection exceeded its absolute lifetime"),
    }
}

#[cfg(test)]
mod tests {
    use super::RejectionCounter;

    #[test]
    fn overload_counter_aggregates_without_per_connection_logging() {
        let counter = RejectionCounter::default();
        counter.record();
        counter.record();
        assert_eq!(counter.total(), 2);
    }
}
