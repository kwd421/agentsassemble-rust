//! Owns runtime construction, joined shutdown and the final handoff decision.
use super::{
    Args, manual_public_ingress_environment, runtime_ready::ControlOwners,
    stable_entry_configuration,
};
use agentsassemble_persistence::{SqliteStore, secure_private_directory};
use agentsassemble_provider::ProviderCatalogService;
use agentsassemble_server::{
    AppState, StableEntryConfig, TicketStore, frontend_release::FrontendRelease,
    local_bind_is_supported, serve,
};
#[cfg(unix)]
use agentsassemble_server::{runtime_control_socket::RuntimeControlSocket, runtime_restart};
use anyhow::Context;
#[cfg(unix)]
use std::os::fd::AsFd;
use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

pub(super) struct StartupExit {
    #[cfg(unix)]
    pub restart: Option<super::runtime_reexec::RuntimeReexec>,
}

struct Startup {
    args: Args,
    state: AppState,
    listener: TcpListener,
    frontend_release: Option<FrontendRelease>,
    #[cfg(unix)]
    private_socket: RuntimeControlSocket,
}

async fn prepare(
    mut args: Args,
    #[cfg(unix)] inherited: Option<super::runtime_reexec::InheritedListeners>,
) -> anyhow::Result<Startup> {
    #[cfg(unix)]
    super::runtime_reexec::validate(&args, inherited.as_ref()).await?;
    #[cfg(not(unix))]
    if args.reexec_operation.is_some()
        || args.reexec_image.is_some()
        || args.reexec_control.is_some()
        || args.frontend_build.is_some()
    {
        anyhow::bail!("runtime listener handoff is unsupported on this platform");
    }
    if args.restart_source.is_none() {
        args.restart_source = Some(std::env::current_exe()?);
    }
    let manual_public_ingress = manual_public_ingress_environment()?;
    let stable_entry = stable_entry_configuration(
        args.stable_entry_config.as_deref(),
        manual_public_ingress.is_some(),
    )?;
    if !local_bind_is_supported(args.bind) {
        anyhow::bail!("the local runtime may bind only to loopback");
    }
    let (store, database_path, frontend_release) = prepare_runtime_storage(&args).await?;
    #[cfg(unix)]
    let (listener, private_socket) = match inherited {
        Some(inherited) => (
            TcpListener::from_std(inherited.http)?,
            RuntimeControlSocket::inherit(
                inherited.control,
                &database_path,
                args.reexec_control
                    .as_deref()
                    .context("missing control custody")?,
            )?,
        ),
        None => (
            TcpListener::bind(args.bind).await?,
            RuntimeControlSocket::bind(&database_path).await?,
        ),
    };
    #[cfg(not(unix))]
    let listener = TcpListener::bind(args.bind).await?;
    let address = listener.local_addr()?;
    args.bind = address;
    args.database.clone_from(&database_path);
    let mut state = AppState::local_with_provider_state_root(
        store,
        TicketStore::new(Duration::from_secs(30), 4_096),
        ProviderCatalogService::discovering(),
        database_state_root(&database_path)?,
    )
    .await?;
    state.google_accounts = agentsassemble_server::GoogleAccountService::from_environment()?;
    state = configure_startup_surface(
        state,
        manual_public_ingress,
        address,
        stable_entry,
        &database_path,
        args.desktop_native_registration,
    )
    .await?;
    if let Some(frontend) = frontend_release.as_ref() {
        state = state.with_frontend(frontend.clone());
    }
    Ok(Startup {
        args,
        state,
        listener,
        frontend_release,
        #[cfg(unix)]
        private_socket,
    })
}

pub(super) async fn run(
    args: Args,
    #[cfg(unix)] inherited: Option<super::runtime_reexec::InheritedListeners>,
) -> anyhow::Result<StartupExit> {
    let Startup {
        args,
        state,
        listener,
        frontend_release,
        #[cfg(unix)]
        private_socket,
    } = prepare(
        args,
        #[cfg(unix)]
        inherited,
    )
    .await?;
    #[cfg(unix)]
    let mut state = state;
    let cancellation = CancellationToken::new();
    #[cfg(unix)]
    let http_descriptor = listener.as_fd().try_clone_to_owned()?;
    #[cfg(unix)]
    let private_listener =
        tokio::net::UnixListener::from_std(private_socket.listener().try_clone()?)?;
    #[cfg(unix)]
    let mut restart_receiver = {
        let (control, receiver) = runtime_restart::RuntimeRestartControl::new(
            args.restart_source
                .clone()
                .context("runtime source missing")?,
            args.frontend.clone(),
            database_state_root(&args.database)?.to_path_buf(),
            cancellation.clone(),
        );
        state.runtime_restart = control;
        receiver
    };
    let signal = cancellation.clone();
    let signal_owner = tokio::spawn(async move {
        tokio::select! {
            () = signal.cancelled() => Ok(()),
            result = tokio::signal::ctrl_c() => {
                signal.cancel();
                result
            },
        }
    });
    let mut controls = ControlOwners::default();
    let serving = Box::pin(serve(
        listener,
        state.clone(),
        cancellation.clone(),
        controls.ready(
            &state,
            &args,
            frontend_release.as_ref(),
            &cancellation,
            #[cfg(unix)]
            private_listener,
        ),
    ))
    .await;
    cancellation.cancel();
    let ready_published = controls.ready_published;
    let control_result = controls.join().await;
    let signal_result = signal_owner.await.context("join signal owner")?;
    if !ready_published && let Some(operation) = args.reexec_operation.as_deref() {
        state
            .store
            .fail_runtime_restart_after_cleanup(operation)
            .await?;
    }
    control_result?;
    signal_result?;
    serving?;
    #[cfg(unix)]
    {
        if let Ok(prepared) = restart_receiver.try_recv() {
            state
                .store
                .begin_runtime_restart_drain(&prepared.operation_id, prepared.image.identity())
                .await?;
            return Ok(StartupExit {
                restart: Some(super::runtime_reexec::RuntimeReexec {
                    prepared,
                    arguments: args,
                    http: http_descriptor,
                    control: private_socket,
                }),
            });
        }
        private_socket.close()?;
    }
    Ok(StartupExit {
        #[cfg(unix)]
        restart: None,
    })
}

async fn prepare_runtime_storage(
    args: &Args,
) -> anyhow::Result<(SqliteStore, PathBuf, Option<FrontendRelease>)> {
    if let Some(parent) = args.database.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .with_context(|| format!("create database directory {}", parent.display()))?;
        secure_private_directory(parent)
            .with_context(|| format!("secure database directory {}", parent.display()))?;
    }
    let store = SqliteStore::open_path(&args.database).await?;
    let database_path = args
        .database
        .canonicalize()
        .with_context(|| format!("resolve database path {}", args.database.display()))?;
    let frontend_release = match (&args.frontend_build, &args.frontend) {
        (Some(build), Some(_)) => Some(FrontendRelease::load(
            database_state_root(&database_path)?,
            build,
        )?),
        (None, Some(source)) => Some(FrontendRelease::materialize(
            source,
            database_state_root(&database_path)?,
        )?),
        (None, None) => None,
        (Some(_), None) => anyhow::bail!("prepared frontend has no source"),
    };
    Ok((store, database_path, frontend_release))
}

fn database_state_root(database: &Path) -> anyhow::Result<&Path> {
    database.parent().context("database path has no state root")
}

async fn configure_startup_surface(
    state: AppState,
    manual: Option<(String, String)>,
    listener: SocketAddr,
    stable_entry: Option<StableEntryConfig>,
    database: &Path,
    central_registration: bool,
) -> anyhow::Result<AppState> {
    let state = match manual {
        Some((origin, proxy_secret)) => {
            state.with_manual_public_ingress(listener, &origin, &proxy_secret)?
        }
        None => {
            state
                .with_managed_public_ingress(listener, stable_entry, database_state_root(database)?)
                .await?
        }
    };
    Ok(if central_registration {
        state.with_central_registration()
    } else {
        state
    })
}
