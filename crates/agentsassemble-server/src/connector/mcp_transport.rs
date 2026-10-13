use rmcp::{
    ServiceExt,
    transport::{stdio, streamable_http_server::StreamableHttpServerConfig},
};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

use super::ConnectorMcp;

/// Runs one current conversation over stdin/stdout until EOF or interrupt.
///
/// The working folder is the CLI's startup directory unless overridden; path
/// confinement for uploads and saved reads is bound to that folder.
///
/// # Errors
/// Reports MCP initialization or transport task failure.
pub async fn serve_stdio(workspace: Option<std::path::PathBuf>) -> anyhow::Result<()> {
    let workspace = workspace
        .map(Ok)
        .unwrap_or_else(std::env::current_dir)
        .map_err(|error| anyhow::anyhow!("connector workspace unavailable: {error}"))?
        .canonicalize()
        .map_err(|error| anyhow::anyhow!("connector workspace unavailable: {error}"))?;
    let identity = agentsassemble_domain::stable_identity_hash(
        &same_file::Handle::from_path(&workspace)
            .map_err(|error| anyhow::anyhow!("connector workspace unavailable: {error}"))?,
    );
    let workspace = workspace
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("connector workspace unavailable"))?
        .to_owned();
    let connector = ConnectorMcp::new(None)
        .map_err(anyhow::Error::msg)?
        .with_workspace(workspace, identity);
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
    let config = StreamableHttpServerConfig::default()
        .with_legacy_session_mode(false)
        .with_json_response(true)
        .with_sse_keep_alive(None)
        .with_allowed_hosts([address.to_string(), format!("localhost:{}", address.port())])
        .with_allowed_origins([
            format!("http://{address}"),
            format!("http://localhost:{}", address.port()),
        ])
        .with_cancellation_token(cancellation.child_token());
    let endpoint = connector.clone();
    let app = axum::Router::new()
        .route(
            "/mcp",
            axum::routing::any(move |request| {
                super::super::mcp_web::serve_request(endpoint.clone(), request, config.clone())
            }),
        )
        .layer(axum::middleware::from_fn(crate::http_body_policy::apply));
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
