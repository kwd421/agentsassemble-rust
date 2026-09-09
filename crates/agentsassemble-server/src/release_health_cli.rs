use agentsassemble_domain::ReleaseHealthStatus;
use agentsassemble_server::release_health;
use clap::Subcommand;
use std::{path::PathBuf, time::Duration};
use tokio_util::sync::CancellationToken;

#[derive(Subcommand)]
pub enum ReleaseHealth {
    /// List fixed local checks; does not run a command.
    List,
    /// Run selected checks and save their latest report for the runtime to display.
    Run {
        #[arg(long, default_value = ".")]
        repository: PathBuf,
        #[arg(long, default_value = ".agentsassemble-rust")]
        output_root: PathBuf,
        #[arg(long = "check")]
        checks: Vec<String>,
        #[arg(long, default_value_t = 300, value_parser = clap::value_parser!(u64).range(1..=3600))]
        timeout_seconds: u64,
    },
}

pub async fn run(command: ReleaseHealth) -> anyhow::Result<()> {
    match command {
        ReleaseHealth::List => println!(
            "{}",
            serde_json::to_string_pretty(&release_health::catalog())?
        ),
        ReleaseHealth::Run {
            repository,
            output_root,
            checks,
            timeout_seconds,
        } => {
            let cancellation = CancellationToken::new();
            let operation = release_health::run(
                &repository,
                &output_root,
                &checks,
                Duration::from_secs(timeout_seconds),
                &cancellation,
            );
            tokio::pin!(operation);
            let report = tokio::select! {
                report = &mut operation => report?,
                signal = tokio::signal::ctrl_c() => {
                    cancellation.cancel();
                    let report = operation.await?;
                    signal?;
                    report
                }
            };
            println!("{}", serde_json::to_string_pretty(&report)?);
            anyhow::ensure!(
                report
                    .results
                    .iter()
                    .all(|result| result.status == ReleaseHealthStatus::Passed),
                "release-health did not pass; the saved report records each outcome"
            );
        }
    }
    Ok(())
}
