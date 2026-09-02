use std::{net::SocketAddr, time::Duration};

use axum::Router;
use hyper::server::conn::http1;
use hyper_util::{
    rt::{TokioIo, TokioTimer},
    service::TowerToHyperService,
};
use tokio::net::TcpStream;
use tokio_util::sync::CancellationToken;

use crate::{
    http_admission::HttpConnectionAdmission,
    ingress_trust::{LocalIngress, PeerAddr},
    public_ingress::PublicIngress,
};

pub(crate) const HTTP_CONNECTION_LIFETIME: Duration = Duration::from_secs(30);
const HTTP_HEADER_TIMEOUT: Duration = Duration::from_secs(3);
const MAX_HTTP_BUFFER_BYTES: usize = 256 * 1024;

pub(crate) async fn serve_connection(
    stream: TcpStream,
    peer: SocketAddr,
    ingress: LocalIngress,
    public_ingress: PublicIngress,
    app: Router,
    admission: HttpConnectionAdmission,
    shutdown: CancellationToken,
) {
    let admission_guard = admission.clone();
    let mut builder = http1::Builder::new();
    builder
        .timer(TokioTimer::new())
        .header_read_timeout(HTTP_HEADER_TIMEOUT)
        .max_buf_size(MAX_HTTP_BUFFER_BYTES);
    let app = app
        .layer(axum::Extension(PeerAddr(peer)))
        .layer(axum::Extension(ingress))
        .layer(axum::Extension(public_ingress))
        .layer(axum::Extension(admission));
    let connection = builder
        .serve_connection(TokioIo::new(stream), TowerToHyperService::new(app))
        .with_upgrades();
    tokio::select! {
        () = shutdown.cancelled() => {}
        result = tokio::time::timeout(HTTP_CONNECTION_LIFETIME, connection) => match result {
            Ok(Ok(())) => {}
            Ok(Err(error)) => tracing::debug!(error = ?error, "HTTP connection closed"),
            Err(_) => tracing::debug!("HTTP connection exceeded its absolute lifetime"),
        }
    }
    drop(admission_guard);
}
