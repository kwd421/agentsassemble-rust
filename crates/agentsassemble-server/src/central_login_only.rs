//! Native authentication listener: no `AppState`, storage, providers or room authority.
use agentsassemble_protocol::{CentralLoginAction, LocalControlRequest, LocalControlResponse};
use agentsassemble_server::CentralLoginService;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

pub(super) async fn run() -> anyhow::Result<()> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let service = CentralLoginService::default();
    let shutdown = CancellationToken::new();
    let serving = tokio::spawn(
        axum::serve(listener, service.clone().router(address.to_string()))
            .with_graceful_shutdown(shutdown.clone().cancelled_owned())
            .into_future(),
    );
    #[cfg(unix)]
    let mut stdin = super::control_input::ControlInput::stdin()?;
    #[cfg(not(unix))]
    let mut stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();
    let result = async {
        super::write_json_line(
            &mut stdout,
            &serde_json::json!({
                "status": "ready", "runtime": "rust", "address": format!("http://{address}"),
                "pid": std::process::id(),
            }),
        )
        .await?;
        let deadline = tokio::time::sleep(Duration::from_mins(10));
        tokio::pin!(deadline);
        loop {
            let line = tokio::select! {
                () = &mut deadline => break,
                line = super::read_control_line(&mut stdin, &shutdown) => line?,
            };
            let Some(line) = line else { break };
            let (response, cancelled) = response(&service, &line).await;
            super::write_json_line(&mut stdout, &response).await?;
            if cancelled {
                // A successful callback redirects to a clean URL and loads its CSS.
                // Retire the state immediately, allowing those requests a bounded drain.
                tokio::time::sleep(Duration::from_secs(2)).await;
                break;
            }
        }
        Ok::<_, anyhow::Error>(())
    }
    .await;
    shutdown.cancel();
    // Bound hostile/stalled HTTP connections during process cleanup.
    let mut serving = serving;
    if let Ok(joined) = tokio::time::timeout(Duration::from_secs(2), &mut serving).await {
        joined??;
    } else {
        serving.abort();
        let _ = serving.await;
    }
    result
}

async fn response(service: &CentralLoginService, line: &[u8]) -> (LocalControlResponse, bool) {
    let (request_id, request) = match super::parse_control_request(line) {
        Ok(request) => request,
        Err(error) => return (*error, false),
    };
    if let LocalControlRequest::CentralLogin { action, state, .. } = request {
        let response = service.control(request_id, action, &state).await;
        let cancelled = action == CentralLoginAction::Cancel
            && matches!(response, LocalControlResponse::CentralLoginOk { .. });
        (response, cancelled)
    } else {
        (
            LocalControlResponse::Error {
                request_id,
                code: "login_only".into(),
                message: "This process accepts only native login returns.".into(),
            },
            false,
        )
    }
}
