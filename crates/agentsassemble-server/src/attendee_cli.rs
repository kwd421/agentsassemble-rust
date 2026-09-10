//! `assemble room attend` owns hidden invitation input and its local provider workspace.
use agentsassemble_provider::{ProviderAdapter, ProviderCatalogService};
use agentsassemble_server::{
    AttendeeRuntime, RoomAttendeeClient, run_attendee_session, shutdown_attendee,
};
use anyhow::Context;
use clap::Args;
use serde_json::json;
use std::{
    io::{BufRead, IsTerminal, Read},
    path::PathBuf,
    time::Duration,
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[derive(Args)]
pub struct Attend {
    #[arg(long)]
    provider: String,
    #[arg(long, default_value = "External Agent")]
    display_name: String,
    #[arg(long)]
    workspace: Option<PathBuf>,
    #[arg(long)]
    model: Option<String>,
    #[arg(long)]
    effort: Option<String>,
    #[arg(long)]
    service_tier: Option<String>,
    #[arg(long)]
    variant: Option<String>,
    #[arg(long, default_value = "meeting_read_only", value_parser = ["meeting_read_only", "workspace_write"])]
    permission_mode: String,
}

pub async fn run(args: Attend) -> anyhow::Result<()> {
    let invite = tokio::task::spawn_blocking(read_invite).await??;
    let mut client = RoomAttendeeClient::new(&invite, &args.provider, &args.display_name)?;
    drop(invite);
    let cancellation = CancellationToken::new();
    let signal = cancellation.clone();
    let signal_task = tokio::spawn(async move {
        let result = shutdown_signal().await;
        signal.cancel();
        result
    });
    let result = Box::pin(run_owned(args, &mut client, &cancellation)).await;
    signal_task.abort();
    match signal_task.await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => return Err(error),
        Err(error) if error.is_cancelled() => {}
        Err(_) => anyhow::bail!("attendee_signal_owner_failed"),
    }
    result
}

async fn run_owned(
    args: Attend,
    client: &mut RoomAttendeeClient,
    cancellation: &CancellationToken,
) -> anyhow::Result<()> {
    let catalog = ProviderCatalogService::discovering_selected(
        &args.provider,
        &agentsassemble_provider::ProviderCredentialStore::production(),
    )?;
    let discovered = discover(&catalog, cancellation).await;
    let shutdown = catalog.shutdown().await;
    shutdown.context("attendee_discovery_shutdown_failed")?;
    discovered?;
    let temporary = if args.workspace.is_none() {
        Some(tempfile::tempdir()?)
    } else {
        None
    };
    let workspace = args
        .workspace
        .as_deref()
        .or_else(|| temporary.as_ref().map(tempfile::TempDir::path))
        .context("attendee_workspace_missing")?;
    if cancellation.is_cancelled() {
        return Ok(());
    }
    // Once admission is sent, recover uncertain commitment with this client's retained secret.
    let joined = tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            match client.join().await {
                Err(error) if error.is_retryable() => {
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
                result => break result,
            }
        }
    })
    .await
    .context("attendee_join_unresolved")??;
    let mut payload = json!({"provider":args.provider, "display_name":args.display_name,
        "catalog_revision":catalog.snapshot().catalog_revision, "workspace":workspace,
        "permission_mode":args.permission_mode});
    for (key, value) in [
        ("model", args.model),
        ("reasoning_effort", args.effort),
        ("service_tier", args.service_tier),
        ("variant", args.variant),
    ] {
        if let Some(value) = value {
            payload[key] = value.into();
        }
    }
    let mut runtime = None;
    let result = async {
        let selection = catalog
            .validate_creation(
                &joined.room_id,
                &joined.participant_id,
                &Uuid::new_v4().to_string(),
                &payload,
            )
            .await?;
        #[cfg(unix)]
        let adapter = ProviderAdapter::with_guardian_executable(&std::env::current_exe()?);
        // The attendee owns its Windows worker and all native descendants through its lease Job.
        #[cfg(not(unix))]
        let adapter = ProviderAdapter::new();
        runtime = Some(AttendeeRuntime::new(
            &joined,
            selection.into(),
            adapter,
            None,
        )?);
        let owned = runtime.as_mut().context("attendee_runtime_missing")?;
        eprintln!("Room admission confirmed; starting the selected provider.");
        Ok::<_, anyhow::Error>(
            run_attendee_session(client, &joined, owned, cancellation, None).await?,
        )
    }
    .await;
    let (stop, failure) = match result {
        Ok(stop) => (stop, None),
        Err(failure) => (None, Some(failure)),
    };
    let cleanup = shutdown_attendee(client, runtime.as_mut(), stop).await;
    if let Err(error) = cleanup {
        if let Some(temporary) = temporary {
            let _retained = temporary.keep();
        }
        if let Some(failure) = failure {
            eprintln!("Attendee run failed: {failure}");
        }
        return Err(error.into());
    }
    if let Some(error) = failure {
        return Err(error);
    }
    eprintln!("Attendee stopped; room cleanup confirmed.");
    Ok(())
}

async fn discover(
    catalog: &ProviderCatalogService,
    cancellation: &CancellationToken,
) -> anyhow::Result<()> {
    let mut updates = catalog.subscribe();
    loop {
        match updates.borrow_and_update().status.as_str() {
            "ready" => return Ok(()),
            "loading" => {}
            _ => anyhow::bail!("attendee_discovery_failed"),
        }
        tokio::select! {
            () = cancellation.cancelled() => anyhow::bail!("attendee_cancelled"),
            result = updates.changed() => result.context("attendee_discovery_unresolved")?,
        }
    }
}

fn read_invite() -> anyhow::Result<String> {
    let value = if std::io::stdin().is_terminal() {
        rpassword::prompt_password("Invite URL: ").context("attendee_invite_input_failed")?
    } else {
        let mut value = String::new();
        std::io::stdin()
            .lock()
            .take(8193)
            .read_line(&mut value)
            .context("attendee_invite_input_failed")?;
        value
    };
    anyhow::ensure!(value.len() <= 8192, "attendee_invite_too_large");
    Ok(value.trim().to_owned())
}

async fn shutdown_signal() -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .context("attendee_signal_unavailable")?;
        tokio::select! {
            result = tokio::signal::ctrl_c() => result.context("attendee_signal_unavailable")?,
            result = terminate.recv() => { result.context("attendee_signal_closed")?; },
        }
    }
    #[cfg(not(unix))]
    tokio::signal::ctrl_c()
        .await
        .context("attendee_signal_unavailable")?;
    Ok(())
}
