use super::*;
use agentsassemble_domain::{AgentRuntimeStatus, RuntimeRestartPhase};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::test]
async fn cancelled_recovery_joins_native_attachment_and_stops_before_the_next_target()
-> anyhow::Result<()> {
    let _serial = AGENT_BOUNDARY_LOCK.lock().await;
    let directory = tempfile::tempdir()?;
    let database = directory.path().join("runtime.sqlite3");
    let starts = directory.path().join("native-starts");
    let gate = TcpListener::bind("127.0.0.1:0").await?;
    let script = recovery_provider(&starts, gate.local_addr()?.port())?;
    let catalog = agent_catalog(directory.path(), Some(script.as_bytes()));
    let store = SqliteStore::open_path(&database).await?;
    bootstrap(&store).await;
    let initial = start(store, catalog.clone()).await;
    create_recovery_targets(&initial.base_url, &initial.state, directory.path()).await?;
    let operation = uuid::Uuid::new_v4().to_string();
    let record = initial
        .state
        .store
        .prepare_runtime_restart(&operation)
        .await?;
    assert_eq!(record.targets.len(), 2);
    initial
        .state
        .store
        .begin_runtime_restart_drain(&operation, "recovery-fixture")
        .await?;
    initial.stop().await;
    assert_eq!(std::fs::read_to_string(&starts)?.lines().count(), 2);

    let store = SqliteStore::open_path(&database).await?;
    let state = AppState::local_with_provider_adapter(
        store.clone(),
        TicketStore::new(Duration::from_secs(30), 16),
        ProviderCatalogService::fixed(catalog),
        ProviderAdapter::with_managed_executable(Path::new(env!(
            "CARGO_BIN_EXE_agentsassemble-server"
        ))),
    )
    .await?;
    let cancellation = CancellationToken::new();
    let recovery_state = state.clone();
    let recovery_signal = cancellation.clone();
    let recovery_operation = operation.clone();
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let task = tokio::spawn(serve(listener, state, cancellation.clone(), async move {
        agentsassemble_server::runtime_restart::recover(
            &recovery_state,
            &recovery_operation,
            "recovery-fixture",
            &recovery_signal,
        )
        .await
        .map_err(std::io::Error::other)
    }));
    let (mut held, _) = tokio::time::timeout(Duration::from_secs(10), gate.accept()).await??;
    let mut marker = [0];
    held.read_exact(&mut marker).await?;
    assert_eq!(&marker, b"R");
    cancellation.cancel();
    assert!(
        store
            .fail_runtime_restart_after_cleanup(&operation)
            .await
            .is_err()
    );
    assert!(
        !task.is_finished(),
        "native cleanup cannot complete while attachment is held"
    );
    held.write_all(b"X").await?;
    assert!(
        tokio::time::timeout(Duration::from_secs(15), task)
            .await??
            .is_err()
    );
    assert_eq!(
        held.read(&mut marker).await?,
        0,
        "the real native provider must exit"
    );
    assert_eq!(
        std::fs::read_to_string(&starts)?.lines().count(),
        3,
        "no second recovery launch"
    );
    for target in &record.targets {
        assert!(
            store
                .load_runtime_reconciliation_candidate(&target.room_id, &target.session_id)
                .await?
                .is_none()
        );
    }
    assert_recovery_finalized(&store, &operation).await
}

async fn assert_recovery_finalized(store: &SqliteStore, operation: &str) -> anyhow::Result<()> {
    let snapshot = store.snapshot("general", 0, 200).await?;
    assert!(snapshot.agent_sessions.iter().all(|session| {
        session.runtime_status == AgentRuntimeStatus::Stopped
            && !session.provider_session_active
            && !session.recovery_required
    }));
    assert_eq!(
        store
            .fail_runtime_restart_after_cleanup(operation)
            .await?
            .phase,
        RuntimeRestartPhase::Failed
    );
    assert_eq!(
        store
            .fail_runtime_restart_after_cleanup(operation)
            .await?
            .phase,
        RuntimeRestartPhase::Failed
    );
    Ok(())
}

fn recovery_provider(starts: &Path, port: u16) -> anyhow::Result<String> {
    Ok(format!(
        r"#!/usr/bin/env python3
import json,os,socket,sys
with open({starts},'a') as f: f.write(str(os.getpid())+'\n')
for line in sys.stdin:
    request=json.loads(line)
    if 'id' not in request: continue
    method=request['method']
    if method=='initialize': result={{}}
    elif method=='thread/start': result={{'thread':{{'id':'thread-'+str(os.getpid())}}}}
    elif method=='thread/resume':
        s=socket.create_connection(('127.0.0.1',{port}))
        s.sendall(b'R')
        if s.recv(1)!=b'X': sys.exit(3)
        result={{'thread':{{'id':request['params']['threadId']}}}}
    else: sys.exit(4)
    print(json.dumps({{'jsonrpc':'2.0','id':request['id'],'result':result}}),flush=True)
",
        starts = serde_json::to_string(&starts)?,
        port = port,
    ))
}

async fn create_recovery_targets(
    base_url: &str,
    state: &AppState,
    workspace: &Path,
) -> anyhow::Result<()> {
    let mut socket = connect(base_url, state).await;
    subscribe(&mut socket).await;
    let _snapshot = receive_json(&mut socket).await;
    for request in ["recovery-first", "recovery-second"] {
        send_create(
            &mut socket,
            request,
            &json!({
                "provider_id":"codex", "catalog_revision":"catalog-boundary-1",
                "display_name":request, "workspace":workspace,
                "model":"gpt-5.6-terra", "permission_mode":"meeting_read_only", "start_now":true,
            }),
        )
        .await;
        let created =
            tokio::time::timeout(Duration::from_secs(10), receive_command_ack(&mut socket)).await?;
        assert_eq!(
            created["result"]["start"]["agent_session"]["runtime_status"],
            "idle"
        );
    }
    drop(socket);
    Ok(())
}
