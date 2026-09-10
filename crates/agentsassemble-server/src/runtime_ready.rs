//! Publishes readiness and owns the parent's and private CLI's admitted response tasks.
use super::{Args, run_control_pipe, write_json_line};
use agentsassemble_server::{AppState, frontend_release::FrontendRelease};
use anyhow::Context;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

#[derive(Default)]
pub(super) struct ControlOwners {
    pipe: Option<JoinHandle<anyhow::Result<()>>>,
    #[cfg(unix)]
    private: Option<JoinHandle<anyhow::Result<()>>>,
    pub ready_published: bool,
}

impl ControlOwners {
    pub async fn ready(
        &mut self,
        state: &AppState,
        args: &Args,
        frontend: Option<&FrontendRelease>,
        cancellation: &CancellationToken,
        #[cfg(unix)] private_listener: tokio::net::UnixListener,
    ) -> std::io::Result<()> {
        #[cfg(unix)]
        let mut stdin = super::control_input::ControlInput::stdin()?;
        #[cfg(not(unix))]
        let mut stdin = tokio::io::stdin();
        #[cfg(unix)]
        Box::pin(stdin.during_recovery(cancellation, async {
            match (
                args.reexec_operation.as_deref(),
                args.reexec_image.as_deref(),
            ) {
                (Some(operation), Some(image)) => {
                    agentsassemble_server::runtime_restart::recover(
                        state,
                        operation,
                        image,
                        cancellation,
                    )
                    .await
                }
                _ => {
                    agentsassemble_server::runtime_restart::recover_abandoned(state, cancellation)
                        .await
                }
            }
            .map_err(std::io::Error::other)
        }))
        .await?;
        if cancellation.is_cancelled() {
            return Err(std::io::Error::other("runtime startup was cancelled"));
        }
        let mut stdout = tokio::io::stdout();
        write_json_line(
            &mut stdout,
            &serde_json::json!({
                "status": "ready", "runtime": "rust", "address": format!("http://{}", args.bind),
                "database": args.database,
                "frontend": frontend.map(FrontendRelease::root),
                "frontend_build_id": frontend.map(FrontendRelease::build_id),
                "pid": std::process::id(),
            }),
        )
        .await
        .map_err(std::io::Error::other)?;
        #[cfg(unix)]
        {
            let state = state.clone();
            let cancellation = cancellation.clone();
            self.private = Some(tokio::spawn(async move {
                let result = agentsassemble_server::runtime_restart_ipc::serve(
                    private_listener,
                    state,
                    cancellation.clone(),
                )
                .await;
                cancellation.cancel();
                result
            }));
        }
        let state = state.clone();
        let cancellation = cancellation.clone();
        self.pipe = Some(tokio::spawn(async move {
            let result = run_control_pipe(&mut stdin, &mut stdout, state, &cancellation).await;
            cancellation.cancel();
            #[cfg(unix)]
            stdin
                .restore()
                .context("restore local control descriptor")?;
            result
        }));
        self.ready_published = true;
        Ok(())
    }

    pub async fn join(self) -> anyhow::Result<()> {
        // Join every admitted response even when another owner has already failed.
        let pipe = match self.pipe {
            Some(owner) => owner
                .await
                .context("join local control pipe")
                .and_then(|result| result),
            None => Ok(()),
        };
        #[cfg(unix)]
        let private = match self.private {
            Some(owner) => owner
                .await
                .context("join private runtime control")
                .and_then(|result| result),
            None => Ok(()),
        };
        pipe?;
        #[cfg(unix)]
        private?;
        Ok(())
    }
}
