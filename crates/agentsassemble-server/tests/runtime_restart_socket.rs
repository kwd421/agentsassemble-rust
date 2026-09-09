#![cfg(unix)]

use agentsassemble_server::{
    AppState,
    runtime_control_socket::RuntimeControlSocket,
    runtime_restart_ipc::{self, RuntimeControlRequest, RuntimeControlResponse},
};
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn private_socket_preserves_active_custody_and_serves_status() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    let database = root.path().join("runtime.sqlite3");
    let store = agentsassemble_persistence::SqliteStore::open_path(&database).await?;
    let socket = RuntimeControlSocket::bind(&database).await?;
    let path = socket
        .listener()
        .local_addr()?
        .as_pathname()
        .ok_or_else(|| anyhow::anyhow!("no socket path"))?
        .to_path_buf();
    assert!(RuntimeControlSocket::bind(&database).await.is_err());
    assert!(path.exists());
    assert!(
        RuntimeControlSocket::inherit(socket.listener().try_clone()?, &database, "wrong").is_err()
    );
    let listener = tokio::net::UnixListener::from_std(socket.listener().try_clone()?)?;
    // Drain the successful active-endpoint probe before the actual protocol case.
    drop(listener.accept().await?);
    let state = AppState::local_with_provider_state_root(
        store,
        agentsassemble_server::TicketStore::new(std::time::Duration::from_secs(30), 16),
        agentsassemble_provider::ProviderCatalogService::discovering_selected(
            "custom_api",
            agentsassemble_provider::ProviderCredentialStore::production(),
        )?,
        root.path(),
        agentsassemble_provider::ProviderCredentialStore::production(),
    )
    .await?;
    let cancellation = CancellationToken::new();
    let server = tokio::spawn(runtime_restart_ipc::serve(
        listener,
        state,
        cancellation.clone(),
    ));
    let result = runtime_restart_ipc::request(
        &database,
        RuntimeControlRequest::Status { operation_id: None },
    )
    .await?;
    assert!(
        matches!(result, RuntimeControlResponse::Snapshot { runtime } if !runtime.supported && runtime.operation.is_none())
    );
    cancellation.cancel();
    server.await??;
    let replacement = RuntimeControlSocket::inherit(
        socket.listener().try_clone()?,
        &database,
        &socket.identity(),
    )?;
    replacement.close()?;
    assert!(!path.exists());
    std::fs::write(&path, b"unrelated replacement")?;
    assert!(socket.close().is_err());
    assert_eq!(std::fs::read(&path)?, b"unrelated replacement");
    std::fs::remove_file(path)?;
    Ok(())
}
