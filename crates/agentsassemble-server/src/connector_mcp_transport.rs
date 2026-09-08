use std::sync::Arc;

use rmcp::{
    ServiceExt,
    transport::{
        stdio,
        streamable_http_server::{
            StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
        },
    },
};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

use super::ConnectorMcp;

/// Runs one current conversation over stdin/stdout until EOF or interrupt.
///
/// # Errors
/// Reports MCP initialization or transport task failure.
pub async fn serve_stdio() -> anyhow::Result<()> {
    let connector = ConnectorMcp::new(None).map_err(anyhow::Error::msg)?;
    let result = async {
        let running = connector.clone().serve(stdio()).await?;
        let cancellation = running.cancellation_token();
        let waiting = running.waiting();
        tokio::pin!(waiting);
        tokio::select! {
            result = &mut waiting => { result?; }
            result = tokio::signal::ctrl_c() => {
                result?;
                connector.close();
                cancellation.cancel();
                waiting.await?;
            }
        }
        Ok(())
    }
    .await;
    connector.close();
    result
}

/// Runs the loopback remote MCP endpoint with exact room-server destinations.
///
/// # Errors
/// Rejects invalid destination lists and reports listener/server failures.
pub async fn serve_remote(port: u16, allowed_servers: Vec<String>) -> anyhow::Result<()> {
    let connector = ConnectorMcp::new(Some(allowed_servers)).map_err(anyhow::Error::msg)?;
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).await?;
    let address = listener.local_addr()?;
    let cancellation = CancellationToken::new();
    let factory = connector.clone();
    let service = StreamableHttpService::new(
        move || Ok(factory.clone()),
        Arc::<LocalSessionManager>::default(),
        StreamableHttpServerConfig::default()
            .with_legacy_session_mode(false)
            .with_json_response(true)
            .with_sse_keep_alive(None)
            .with_allowed_hosts([address.to_string(), format!("localhost:{}", address.port())])
            .with_allowed_origins([
                format!("http://{address}"),
                format!("http://localhost:{}", address.port()),
            ])
            .with_max_request_body_bytes(64 * 1024)
            .with_cancellation_token(cancellation.child_token()),
    );
    let app = axum::Router::new().nest_service("/mcp", service);
    let shutdown_connector = connector.clone();
    let shutdown_cancellation = cancellation.clone();
    let result = axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            if tokio::signal::ctrl_c().await.is_err() {
                tracing::error!("connector interrupt handler failed");
            }
            shutdown_connector.close();
            shutdown_cancellation.cancel();
        })
        .await;
    connector.close();
    cancellation.cancel();
    result.map_err(Into::into)
}
