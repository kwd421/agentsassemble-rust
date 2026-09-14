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
    tokio::pin!(connection);
    let serving = async {
        tokio::select! {
            () = shutdown.cancelled() => {
                connection.as_mut().graceful_shutdown();
                (&mut connection).await
            }
            result = &mut connection => result,
        }
    };
    match tokio::time::timeout(HTTP_CONNECTION_LIFETIME, serving).await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => tracing::debug!(error = ?error, "HTTP connection closed"),
        Err(_) if admission_guard.has_authenticated_wait() => {
            // Stop accepting another request on this connection. The admitted wait
            // retains its own session expiry/revocation/shutdown lifetime.
            connection.as_mut().graceful_shutdown();
            tokio::select! {
                result = &mut connection => {
                    if let Err(error) = result {
                        tracing::debug!(error = ?error, "Authenticated HTTP wait closed");
                    }
                }
                () = admission_guard.authenticated_wait_finished() => {
                    match tokio::time::timeout(HTTP_CONNECTION_LIFETIME, &mut connection).await {
                        Ok(Ok(())) => {}
                        Ok(Err(error)) => tracing::debug!(error = ?error, "Authenticated HTTP response closed"),
                        Err(_) => tracing::debug!("Authenticated HTTP response exceeded its flush lifetime"),
                    }
                }
            }
        }
        Err(_) => tracing::debug!("HTTP connection exceeded its absolute lifetime"),
    }
    drop(admission_guard);
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::routing::get;
    use std::sync::Arc;
    use tokio::{net::TcpListener, sync::Notify};

    #[tokio::test]
    async fn only_the_active_authenticated_wait_outlives_the_absolute_deadline()
    -> Result<(), Box<dyn std::error::Error>> {
        for authenticated in [false, true] {
            let entered = Arc::new(Notify::new());
            let released = Arc::new(Notify::new());
            let handler_entered = entered.clone();
            let handler_released = released.clone();
            let app = Router::new().route(
                "/waiting",
                get(
                    move |axum::Extension(admission): axum::Extension<HttpConnectionAdmission>| {
                        let entered = handler_entered.clone();
                        let released = handler_released.clone();
                        async move {
                            let _lease =
                                authenticated.then(|| admission.retain_authenticated_wait());
                            entered.notify_one();
                            released.notified().await;
                            "wait response"
                        }
                    },
                ),
            );
            let listener = TcpListener::bind("127.0.0.1:0").await?;
            let address = listener.local_addr()?;
            let client = tokio::spawn(async move {
                reqwest::Client::new()
                    .get(format!("http://{address}/waiting"))
                    .timeout(Duration::from_secs(90))
                    .send()
                    .await?
                    .text()
                    .await
            });
            let (stream, peer) = listener.accept().await?;
            let admission = crate::http_admission::HttpAdmission::default()
                .admit()
                .ok_or("fixture admission rejected")?;
            let observation = admission.clone();
            let mut serving = Box::pin(serve_connection(
                stream,
                peer,
                LocalIngress::from_listener(address).ok_or("listener")?,
                PublicIngress::disabled(),
                app,
                admission,
                CancellationToken::new(),
            ));
            tokio::select! {
                () = entered.notified() => {},
                () = &mut serving => panic!("ended before admitted handler"),
            }
            tokio::time::pause();
            tokio::time::advance(HTTP_CONNECTION_LIFETIME + Duration::from_secs(1)).await;
            if authenticated {
                assert!(futures_util::poll!(&mut serving).is_pending());
                released.notify_one();
                let ((), response) = tokio::join!(serving, client);
                assert_eq!(response??, "wait response");
                assert!(!observation.has_authenticated_wait());
            } else {
                serving.await;
                assert!(client.await?.is_err());
            }
            tokio::time::resume();
        }
        Ok(())
    }

    #[tokio::test]
    async fn shutdown_finishes_the_admitted_http_response() -> Result<(), Box<dyn std::error::Error>>
    {
        let entered = Arc::new(Notify::new());
        let released = Arc::new(Notify::new());
        let handler_entered = entered.clone();
        let handler_released = released.clone();
        let app = Router::new().route(
            "/accepted",
            get(move || {
                let entered = handler_entered.clone();
                let released = handler_released.clone();
                async move {
                    entered.notify_one();
                    released.notified().await;
                    "accepted response"
                }
            }),
        );
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let client = tokio::spawn(async move {
            reqwest::get(format!("http://{address}/accepted"))
                .await?
                .text()
                .await
        });
        let (stream, peer) = listener.accept().await?;
        let admission = crate::http_admission::HttpAdmission::default()
            .admit()
            .ok_or("fixture admission rejected")?;
        let shutdown = CancellationToken::new();
        let mut serving = Box::pin(serve_connection(
            stream,
            peer,
            LocalIngress::from_listener(address).ok_or("invalid fixture listener")?,
            PublicIngress::disabled(),
            app,
            admission,
            shutdown.clone(),
        ));
        tokio::select! {
            () = entered.notified() => {},
            () = &mut serving => panic!("connection ended before the handler was admitted"),
        }
        shutdown.cancel();
        assert!(
            futures_util::poll!(&mut serving).is_pending(),
            "shutdown discarded the admitted response"
        );
        released.notify_one();
        let ((), result) = tokio::join!(serving, client);
        assert_eq!(result??, "accepted response");
        Ok(())
    }
}
