#![cfg(unix)]
use agentsassemble_domain::{LocalAttendeeCreate, LocalAttendeePhase as Phase};
use agentsassemble_provider::{ProviderAdapter, ProviderCatalogService};
use agentsassemble_server::LocalAttendeeService;
use serde_json::json;
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[path = "support/attendee.rs"]
mod attendee;
#[path = "support/human_invite.rs"]
mod human_invite;
#[path = "support/provider_fixture.rs"]
mod provider_fixture;
#[path = "support/room_socket_peer.rs"]
mod room_socket_peer;

fn adapter() -> ProviderAdapter {
    ProviderAdapter::with_guardian_executable(Path::new(env!(
        "CARGO_BIN_EXE_agentsassemble-server"
    )))
}

fn request(
    base: &str,
    invite: &agentsassemble_persistence::AttendeeInvite,
    workspace: &Path,
) -> LocalAttendeeCreate {
    LocalAttendeeCreate {
        request_id: Uuid::new_v4(),
        invite_url: format!("{base}/join?token={}", invite.invite_bearer),
        room_id: "general".to_owned(),
        room_uid: invite.room_uid,
        creation: json!({"provider_id":"codex", "display_name":"Own computer draft", "workspace":workspace,
            "catalog_revision":"catalog-boundary-1", "start":false}),
    }
}

#[tokio::test]
async fn own_catalog_add_only_start_and_exact_cleanup_do_not_use_the_room_host_catalog()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let catalog =
        ProviderCatalogService::fixed(provider_fixture::agent_catalog(directory.path(), None));
    let (store, invite) = attendee::fixture().await?;
    let server = human_invite::start(store.clone()).await;
    let host_catalog = server.state().provider_catalog.snapshot();
    assert!(host_catalog.providers.is_empty());
    let service = LocalAttendeeService::new(store.clone(), CancellationToken::new());
    let request = request(&server.base_url, &invite, directory.path());
    let admitted = service
        .create(request.clone(), catalog.clone(), adapter())
        .await?;
    assert_eq!(admitted.phase, Phase::Admitted);
    assert_eq!(
        service
            .create(request.clone(), catalog.clone(), adapter())
            .await?,
        admitted
    );
    assert!(!store.snapshot("general", 0, 200).await?.agent_sessions[0].provider_session_active);
    let mut changed = request.clone();
    changed.creation["display_name"] = "Changed retry".into();
    assert_eq!(
        service
            .create(changed, catalog.clone(), adapter())
            .await
            .err()
            .ok_or("expected retained failure")?
            .code,
        "local_attendee_request_changed"
    );
    let mut duplicate = request.clone();
    duplicate.request_id = Uuid::new_v4();
    assert_eq!(
        service
            .create(duplicate, catalog.clone(), adapter())
            .await
            .err()
            .ok_or("expected retained failure")?
            .code,
        "local_attendee_invitation_owned"
    );
    let running = service.start(request.request_id).await?;
    assert_eq!(running.phase, Phase::Running);
    assert_eq!(running.participant_id, admitted.participant_id);
    let snapshot = store.snapshot("general", 0, 200).await?;
    assert_eq!(snapshot.agent_sessions.len(), 1);
    assert!(snapshot.agent_sessions[0].provider_session_active);
    assert_eq!(snapshot.agent_sessions[0].model, "gpt-5.6-terra");
    assert_eq!(
        server.state().provider_catalog.snapshot().catalog_revision,
        host_catalog.catalog_revision
    );
    assert_eq!(
        service.cancel(request.request_id).await?.phase,
        Phase::Stopped
    );
    assert_eq!(
        service.cancel(request.request_id).await?.phase,
        Phase::Stopped
    );
    service.shutdown().await?;
    service.shutdown().await?;
    let restarted = LocalAttendeeService::new(store.clone(), CancellationToken::new());
    let restored = restarted.status(request.request_id).await?;
    assert_eq!(restored.phase, Phase::CleanupUnconfirmed);
    assert_eq!(
        restored.error_code.as_deref(),
        Some("local_attendee_process_restarted")
    );
    assert_eq!(
        restarted
            .create(request, catalog, adapter())
            .await
            .err()
            .ok_or("expected receipt conflict")?
            .code,
        "local_attendee_receipt_unavailable"
    );
    assert!(!store.snapshot("general", 0, 200).await?.agent_sessions[0].provider_session_active);
    server.stop().await;
    Ok(())
}

#[tokio::test]
async fn self_targeted_attendees_finish_cleanup_before_their_server_closes_ingress()
-> Result<(), Box<dyn std::error::Error>> {
    for start in [false, true] {
        let directory = tempfile::tempdir()?;
        let catalog =
            ProviderCatalogService::fixed(provider_fixture::agent_catalog(directory.path(), None));
        let (store, invite) = attendee::fixture().await?;
        let server = human_invite::start(store.clone()).await;
        let service = server.state().local_attendees.clone();
        // The add-only relay creates a fresh upstream client for every request, so
        // cleanup cannot succeed by reusing the admission connection after accept closes.
        let relay = Relay::start(&server.base_url, false, false).await?;
        let base = if start { &server.base_url } else { &relay.base };
        let mut request = request(base, &invite, directory.path());
        request.creation["start"] = start.into();
        let created = service
            .create(request.clone(), catalog.clone(), adapter())
            .await?;
        assert_eq!(
            created.phase,
            if start {
                Phase::Running
            } else {
                Phase::Admitted
            }
        );
        // No explicit attendee cancel: this is the actual AppState shutdown owner.
        server.stop().await;
        assert_eq!(
            service.status(request.request_id).await?.phase,
            Phase::Stopped
        );
        let snapshot = store.snapshot("general", 0, 200).await?;
        assert_eq!(snapshot.agent_sessions.len(), 1);
        assert!(!snapshot.agent_sessions[0].provider_session_active);
        assert!(!snapshot.agent_sessions[0].recovery_required);
        assert_eq!(
            service
                .create(request, catalog, adapter())
                .await
                .err()
                .ok_or("expected closed local creation")?
                .code,
            "local_attendee_closed"
        );
        if !start {
            // One cleanup read and its one exact completion report.
            assert_eq!(relay.gate.cleanups.load(Ordering::SeqCst), 2);
        }
        relay.stop().await?;
    }
    Ok(())
}

#[tokio::test]
async fn held_native_start_can_be_cancelled_through_private_http_without_waiting_for_readiness()
-> Result<(), Box<dyn std::error::Error>> {
    use tokio::io::AsyncReadExt;
    for create_and_start in [false, true] {
        let directory = tempfile::tempdir()?;
        let barrier = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let fixture = format!(
            "#!/usr/bin/env python3\nimport socket,sys\nsys.stdin.readline()\ns=socket.create_connection(('127.0.0.1',{}))\ns.sendall(b'1')\ns.recv(1)\n",
            barrier.local_addr()?.port()
        );
        let catalog = ProviderCatalogService::fixed(provider_fixture::agent_catalog(
            directory.path(),
            Some(fixture.as_bytes()),
        ));
        let (store, invite) = attendee::fixture().await?;
        let server = human_invite::start(store.clone()).await;
        let service = server.state().local_attendees.clone();
        let mut input = request(&server.base_url, &invite, directory.path());
        input.creation["start"] = create_and_start.into();
        let id = input.request_id;
        if !create_and_start {
            assert_eq!(
                service
                    .create(input.clone(), catalog.clone(), adapter())
                    .await?
                    .phase,
                Phase::Admitted
            );
        }
        let owner = service.clone();
        let pending = tokio::spawn(async move {
            if create_and_start {
                owner.create(input, catalog, adapter()).await
            } else {
                owner.start(id).await
            }
        });
        let (mut gate, _) =
            tokio::time::timeout(std::time::Duration::from_secs(10), barrier.accept()).await??;
        assert_eq!(gate.read_u8().await?, b'1');
        assert!(!pending.is_finished());
        let ticket = server
            .state()
            .tickets
            .issue_server_operator(agentsassemble_domain::LOCAL_OPERATOR_USER_ID.to_owned())
            .await?
            .ticket;
        let response = reqwest::Client::new()
            .post(format!("{}/api/local-attendees/{id}", server.base_url))
            .bearer_auth(ticket)
            .json(&json!({"action":"cancel"}))
            .send()
            .await?;
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        let stopped: agentsassemble_domain::LocalAttendeeStatus = response.json().await?;
        assert_eq!(stopped.phase, Phase::Stopped);
        assert_eq!(pending.await??.phase, Phase::Stopped);
        assert_eq!(
            gate.read(&mut [0]).await?,
            0,
            "the held native process must be gone"
        );
        let snapshot = store.snapshot("general", 0, 200).await?;
        assert_eq!(snapshot.agent_sessions.len(), 1);
        assert!(!snapshot.agent_sessions[0].provider_session_active);
        assert!(!snapshot.agent_sessions[0].recovery_required);
        server.stop().await;
    }
    Ok(())
}

#[tokio::test]
async fn dropped_waiter_retains_lost_admission_for_read_only_observation_and_exact_retry()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let catalog =
        ProviderCatalogService::fixed(provider_fixture::agent_catalog(directory.path(), None));
    let (store, invite) = attendee::fixture().await?;
    let server = human_invite::start(store.clone()).await;
    let relay = Relay::start(&server.base_url, true, false).await?;
    let service = LocalAttendeeService::new(store.clone(), CancellationToken::new());
    let request = request(&relay.base, &invite, directory.path());
    let (owner, input, models) = (service.clone(), request.clone(), catalog.clone());
    let waiter = tokio::spawn(async move { owner.create(input, models, adapter()).await });
    relay.gate.entered.notified().await;
    waiter.abort();
    assert!(
        waiter
            .await
            .err()
            .ok_or("expected retained failure")?
            .is_cancelled()
    );
    relay.gate.release.notify_one();
    assert_eq!(
        service
            .create(request.clone(), catalog, adapter())
            .await?
            .phase,
        Phase::AdmissionUnresolved
    );
    assert_eq!(
        service.status(request.request_id).await?.phase,
        Phase::AdmissionUnresolved
    );
    assert_eq!(relay.gate.joins.load(Ordering::SeqCst), 1);
    let recovered = service.retry(request.request_id).await?;
    assert_eq!(recovered.phase, Phase::Admitted);
    assert_eq!(relay.gate.joins.load(Ordering::SeqCst), 2);
    assert_eq!(
        store
            .snapshot("general", 0, 200)
            .await?
            .agent_sessions
            .len(),
        1
    );
    assert_eq!(
        service.cancel(request.request_id).await?.phase,
        Phase::Stopped
    );
    service.shutdown().await?;
    relay.stop().await?;
    server.stop().await;
    Ok(())
}

#[tokio::test]
async fn remote_cleanup_failure_is_retained_across_cancel_and_concurrent_shutdown()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let catalog =
        ProviderCatalogService::fixed(provider_fixture::agent_catalog(directory.path(), None));
    let (store, invite) = attendee::fixture().await?;
    let server = human_invite::start(store.clone()).await;
    let relay = Relay::start(&server.base_url, false, true).await?;
    let service = LocalAttendeeService::new(store.clone(), CancellationToken::new());
    let request = request(&relay.base, &invite, directory.path());
    service.create(request.clone(), catalog, adapter()).await?;
    let failure = service
        .cancel(request.request_id)
        .await
        .err()
        .ok_or("expected retained failure")?
        .code;
    assert_eq!(
        service.status(request.request_id).await?.phase,
        Phase::CleanupUnconfirmed
    );
    let (first, second) = tokio::join!(service.shutdown(), service.shutdown());
    assert_eq!(
        first.err().ok_or("expected retained failure")?.code,
        failure
    );
    assert_eq!(
        second.err().ok_or("expected retained failure")?.code,
        failure
    );
    assert_eq!(
        service
            .cancel(request.request_id)
            .await
            .err()
            .ok_or("expected retained failure")?
            .code,
        failure
    );
    assert_eq!(relay.gate.cleanups.load(Ordering::SeqCst), 1);
    relay.stop().await?;
    server.stop().await;
    Ok(())
}

struct Gate {
    upstream: String,
    lose_first: AtomicBool,
    deny_cleanup: bool,
    joins: AtomicUsize,
    cleanups: AtomicUsize,
    entered: Notify,
    release: Notify,
}

struct Relay {
    base: String,
    gate: Arc<Gate>,
    cancel: CancellationToken,
    task: tokio::task::JoinHandle<std::io::Result<()>>,
}

impl Relay {
    async fn start(
        upstream: &str,
        lose_first: bool,
        deny_cleanup: bool,
    ) -> Result<Self, std::io::Error> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let base = format!("http://{}", listener.local_addr()?);
        let gate = Arc::new(Gate {
            upstream: upstream.to_owned(),
            lose_first: AtomicBool::new(lose_first),
            deny_cleanup,
            joins: AtomicUsize::new(0),
            cleanups: AtomicUsize::new(0),
            entered: Notify::new(),
            release: Notify::new(),
        });
        let app = axum::Router::new()
            .fallback(forward)
            .with_state(gate.clone());
        let cancel = CancellationToken::new();
        let stopping = cancel.clone();
        let task = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(stopping.cancelled_owned())
                .await
        });
        Ok(Self {
            base,
            gate,
            cancel,
            task,
        })
    }

    async fn stop(self) -> Result<(), Box<dyn std::error::Error>> {
        self.cancel.cancel();
        self.task.await??;
        Ok(())
    }
}

async fn forward(
    axum::extract::State(gate): axum::extract::State<Arc<Gate>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    use axum::body::Body;
    let (mut parts, body) = request.into_parts();
    let path = parts.uri.path();
    if path.ends_with("/cleanup") {
        gate.cleanups.fetch_add(1, Ordering::SeqCst);
        if gate.deny_cleanup {
            return axum::response::Response::builder()
                .status(403)
                .body(Body::from(
                    r#"{"error":{"code":"attendee_cleanup_rejected"}}"#,
                ))
                .unwrap_or_else(|_| panic!("controlled attendee relay failed"));
        }
    }
    let join = path.ends_with("/join");
    if join {
        gate.joins.fetch_add(1, Ordering::SeqCst);
    }
    parts.headers.remove("host");
    let body = axum::body::to_bytes(body, 65536)
        .await
        .unwrap_or_else(|_| panic!("controlled attendee relay failed"));
    let response = reqwest::Client::new()
        .request(parts.method, format!("{}{}", gate.upstream, parts.uri))
        .headers(parts.headers)
        .body(body)
        .send()
        .await
        .unwrap_or_else(|_| panic!("controlled attendee relay failed"));
    let status = response.status();
    let bytes = response
        .bytes()
        .await
        .unwrap_or_else(|_| panic!("controlled attendee relay failed"));
    let body = if join && gate.lose_first.swap(false, Ordering::SeqCst) {
        gate.entered.notify_one();
        gate.release.notified().await;
        Body::from("{")
    } else {
        Body::from(bytes)
    };
    axum::response::Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(body)
        .unwrap_or_else(|_| panic!("controlled attendee relay failed"))
}
