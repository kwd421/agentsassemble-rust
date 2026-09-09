use std::{path::PathBuf, time::Duration};

#[derive(clap::Args)]
pub struct RuntimeRestart {
    #[arg(long, default_value = ".agentsassemble-rust/runtime.sqlite3")]
    database: PathBuf,
    /// Query a receipt instead of requesting a new restart.
    #[arg(long)]
    status: bool,
    /// Reuse this operation ID after an uncertain response, or select its status.
    #[arg(long)]
    operation_id: Option<uuid::Uuid>,
    /// Wait for busy work to end and observe completion within this many seconds.
    #[arg(long, default_value = "0", value_parser = parse_wait)]
    wait: Duration,
    #[arg(long, visible_alias = "as-json")]
    json: bool,
}

fn parse_wait(value: &str) -> Result<Duration, String> {
    value
        .parse::<f64>()
        .map_err(|error| error.to_string())
        .and_then(|seconds| Duration::try_from_secs_f64(seconds).map_err(|error| error.to_string()))
}

#[cfg(not(unix))]
pub async fn run(_args: RuntimeRestart) -> anyhow::Result<()> {
    anyhow::bail!("rolling restart is unsupported on this platform")
}

#[cfg(unix)]
use agentsassemble_domain::{RuntimeRestartPhase, RuntimeRestartReceipt};
#[cfg(unix)]
use agentsassemble_server::runtime_restart_ipc::{
    self, RuntimeControlRequest, RuntimeControlResponse,
};

#[cfg(unix)]
pub async fn run(args: RuntimeRestart) -> anyhow::Result<()> {
    use anyhow::Context;
    let deadline = tokio::time::Instant::now()
        .checked_add(args.wait)
        .context("wait exceeds the clock range")?;
    let response = if args.status {
        runtime_restart_ipc::request(
            &args.database,
            RuntimeControlRequest::Status {
                operation_id: args.operation_id,
            },
        )
        .await?
    } else {
        let operation_id = args.operation_id.unwrap_or_else(uuid::Uuid::new_v4);
        eprintln!("Restart operation: {operation_id}");
        loop {
            let response = runtime_restart_ipc::request(&args.database, RuntimeControlRequest::Restart { operation_id }).await
                .with_context(|| format!("restart {operation_id} response unavailable; query the same operation ID to resolve its outcome"))?;
            if matches!(&response, RuntimeControlResponse::Error { code, .. } if code == "runtime_restart_busy")
                && tokio::time::Instant::now() < deadline
            {
                eprintln!("Waiting for the runtime to become idle...");
                tokio::time::sleep_until(
                    deadline.min(tokio::time::Instant::now() + Duration::from_secs(2)),
                )
                .await;
                if tokio::time::Instant::now() < deadline {
                    continue;
                }
            }
            break response;
        }
    };
    if args.json {
        println!("{}", serde_json::to_string_pretty(&response)?);
    }
    let receipt = match response {
        RuntimeControlResponse::Accepted { operation } => Some(operation),
        RuntimeControlResponse::Snapshot { runtime } => {
            if !runtime.supported {
                anyhow::bail!("rolling restart is unsupported by this runtime");
            }
            runtime.operation
        }
        RuntimeControlResponse::Error { code, message } => anyhow::bail!("{code}: {message}"),
    };
    if let Some(receipt) = receipt {
        if !args.json {
            report(&receipt, false)?;
        }
        if receipt.phase.blocks_admission() {
            if tokio::time::Instant::now() < deadline {
                return wait(
                    &args.database,
                    uuid::Uuid::parse_str(&receipt.operation_id)?,
                    deadline,
                    args.json,
                )
                .await;
            }
        } else {
            require_completed(receipt.phase)?;
        }
    } else if !args.json {
        println!("No restart operation is recorded.");
    }
    Ok(())
}

#[cfg(unix)]
fn report(receipt: &RuntimeRestartReceipt, json: bool) -> anyhow::Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(receipt)?);
    } else {
        println!("Restart {}: {:?}", receipt.operation_id, receipt.phase);
    }
    Ok(())
}

#[cfg(unix)]
fn require_completed(phase: RuntimeRestartPhase) -> anyhow::Result<()> {
    if phase != RuntimeRestartPhase::Completed {
        anyhow::bail!("restart did not complete: {phase:?}");
    }
    Ok(())
}

#[cfg(unix)]
async fn wait(
    database: &std::path::Path,
    operation_id: uuid::Uuid,
    deadline: tokio::time::Instant,
    json: bool,
) -> anyhow::Result<()> {
    let terminal = tokio::time::timeout_at(deadline, async {
        loop {
            // This selected in-progress operation owns the bounded observation loop.
            tokio::time::sleep(Duration::from_secs(1)).await;
            match runtime_restart_ipc::request(
                database,
                RuntimeControlRequest::Status {
                    operation_id: Some(operation_id),
                },
            )
            .await
            {
                Ok(RuntimeControlResponse::Snapshot { runtime }) => {
                    let receipt = runtime
                        .operation
                        .ok_or_else(|| anyhow::anyhow!("restart receipt is missing"))?;
                    if !receipt.phase.blocks_admission() {
                        return Ok(receipt);
                    }
                }
                Ok(RuntimeControlResponse::Error { code, message }) => {
                    anyhow::bail!("{code}: {message}")
                }
                Ok(RuntimeControlResponse::Accepted { .. }) => {
                    anyhow::bail!("unexpected restart status response")
                }
                Err(error) => {
                    eprintln!("Restart {operation_id} outcome is not yet available: {error}");
                }
            }
        }
    })
    .await
    .map_err(|_| {
        anyhow::anyhow!(
            "restart {operation_id} outcome is unknown at the deadline; query the same operation ID"
        )
    })??;
    report(&terminal, json)?;
    require_completed(terminal.phase)
}
