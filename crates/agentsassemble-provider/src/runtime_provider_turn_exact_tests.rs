use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use tokio::sync::Barrier;

use super::{
    DriverError, DriverFactory, DriverFuture, ProviderAdapter, ProviderDriver,
    ProviderRuntimeObservation, ProviderSessionAttachment, ProviderTurnCompleted,
    ProviderTurnRequest,
    provider_turn_tests::{active_session, requests, stop_and_release, turn_fixture},
    tests::fixture_session,
};
use crate::{launch_error::DriverLaunchError, runtime_lease::HeldRuntimeLease};

struct PanicAfterIoFactory {
    turn_entries: Arc<AtomicUsize>,
    stops: Arc<AtomicUsize>,
}

struct PanicAfterIoDriver {
    turn_entries: Arc<AtomicUsize>,
    stops: Arc<AtomicUsize>,
}

struct DefinitiveFailureFactory;

struct DefinitiveFailureDriver;

impl DriverFactory for DefinitiveFailureFactory {
    fn launch<'a>(
        &'a self,
        _session: &'a agentsassemble_domain::DurableAgentSession,
        _runtime_lease: &'a HeldRuntimeLease,
    ) -> DriverFuture<'a, Result<Box<dyn ProviderDriver>, DriverLaunchError>> {
        Box::pin(async { Ok(Box::new(DefinitiveFailureDriver) as Box<dyn ProviderDriver>) })
    }
}

impl ProviderDriver for DefinitiveFailureDriver {
    fn attach_session<'a>(
        &'a mut self,
        session: &'a agentsassemble_domain::DurableAgentSession,
    ) -> DriverFuture<'a, Result<ProviderSessionAttachment, DriverError>> {
        let model = session.public.model.clone();
        Box::pin(async move {
            Ok(ProviderSessionAttachment {
                provider_session_id: "definitive-failure-session".to_owned(),
                reused: false,
                observed_model_id: Some(model),
            })
        })
    }

    fn send_turn<'a>(
        &'a mut self,
        _session: &'a agentsassemble_domain::DurableAgentSession,
        _request: &'a ProviderTurnRequest,
    ) -> DriverFuture<'a, Result<ProviderTurnCompleted, DriverError>> {
        Box::pin(async {
            Err(DriverError::new(
                "provider_credential_rejected",
                "The provider rejected the configured credential.",
            ))
        })
    }

    fn is_alive(&mut self) -> DriverFuture<'_, Result<bool, DriverError>> {
        Box::pin(async { Ok(true) })
    }

    fn stop(&mut self) -> DriverFuture<'_, Result<(), DriverError>> {
        Box::pin(async { Ok(()) })
    }

    fn turn_failure_effect_uncertain(&self) -> bool {
        false
    }
}

impl DriverFactory for PanicAfterIoFactory {
    fn launch<'a>(
        &'a self,
        _session: &'a agentsassemble_domain::DurableAgentSession,
        _runtime_lease: &'a HeldRuntimeLease,
    ) -> DriverFuture<'a, Result<Box<dyn ProviderDriver>, DriverLaunchError>> {
        let driver = PanicAfterIoDriver {
            turn_entries: Arc::clone(&self.turn_entries),
            stops: Arc::clone(&self.stops),
        };
        Box::pin(async move { Ok(Box::new(driver) as Box<dyn ProviderDriver>) })
    }
}

impl ProviderDriver for PanicAfterIoDriver {
    fn attach_session<'a>(
        &'a mut self,
        session: &'a agentsassemble_domain::DurableAgentSession,
    ) -> DriverFuture<'a, Result<ProviderSessionAttachment, DriverError>> {
        let model = session.public.model.clone();
        Box::pin(async move {
            Ok(ProviderSessionAttachment {
                provider_session_id: "panic-after-io-session".to_owned(),
                reused: false,
                observed_model_id: Some(model),
            })
        })
    }

    fn send_turn<'a>(
        &'a mut self,
        _session: &'a agentsassemble_domain::DurableAgentSession,
        _request: &'a ProviderTurnRequest,
    ) -> DriverFuture<'a, Result<ProviderTurnCompleted, DriverError>> {
        let turn_entries = Arc::clone(&self.turn_entries);
        Box::pin(async move {
            turn_entries.fetch_add(1, Ordering::SeqCst);
            tokio::task::yield_now().await;
            panic!("synthetic panic after provider I/O entry");
        })
    }

    fn is_alive(&mut self) -> DriverFuture<'_, Result<bool, DriverError>> {
        Box::pin(async { Ok(true) })
    }

    fn stop(&mut self) -> DriverFuture<'_, Result<(), DriverError>> {
        let stops = Arc::clone(&self.stops);
        Box::pin(async move {
            stops.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
    }
}

#[tokio::test]
async fn completed_turn_remains_exactly_owned_until_durable_terminal_release() {
    let _serial = super::tests::RUNTIME_TEST_LOCK.lock().await;
    let directory = tempfile::tempdir()
        .unwrap_or_else(|error| panic!("create retained-result fixture: {error}"));
    let transcript = directory.path().join("requests.jsonl");
    let script = turn_fixture(
        &transcript,
        "",
        "{\"jsonrpc\":\"2.0\",\"id\":3,\"result\":{\"turn\":{\"id\":\"provider-turn-1\"}}}",
        concat!(
            "printf '%s\n' '{\"jsonrpc\":\"2.0\",\"method\":\"agent_message/completed\",\"params\":{\"threadId\":\"thread-1\",\"turnId\":\"provider-turn-1\",\"text\":\"retained answer\"}}'\n",
            "printf '%s\n' '{\"jsonrpc\":\"2.0\",\"method\":\"turn/completed\",\"params\":{\"threadId\":\"thread-1\",\"turnId\":\"provider-turn-1\"}}'\n",
        ),
    );
    let session = fixture_session(directory.path(), &script).await;
    let adapter = ProviderAdapter::new();
    let started = adapter
        .start(&session)
        .await
        .unwrap_or_else(|error| panic!("start retained-result fixture: {error}"));
    let active = active_session(&session, &started, "room-turn-1");
    let request = ProviderTurnRequest {
        turn_id: "room-turn-1".to_owned(),
        turn_generation: 1,
        execution_id: "11111111-1111-4111-8111-111111111111".to_owned(),
        input: "Retain this result until its durable commit.".to_owned(),
        room_observation: None,
    };
    let prepared = adapter
        .prepare_turn(&active, &request)
        .await
        .unwrap_or_else(|error| panic!("prepare retained result: {error}"));
    let authority = prepared.exact_authority();
    let completed = adapter
        .send_prepared_turn(prepared, &active, &request)
        .await
        .unwrap_or_else(|error| panic!("complete retained result: {error}"));

    assert!(adapter.owns_exact_turn(&authority).await);
    assert_eq!(
        adapter.retained_turn_result(&authority).await,
        Some(Ok(completed))
    );
    let control = adapter
        .begin_exact_turn(&authority)
        .await
        .unwrap_or_else(|error| panic!("resolve quiesced exact result: {error}"));
    assert_eq!(
        control.disposition,
        super::ProviderTurnInterruptDisposition::Quiesced
    );

    adapter.release_terminal_turn(&authority).await;
    assert!(!adapter.owns_exact_turn(&authority).await);
    let Err(error) = adapter.begin_exact_turn(&authority).await else {
        panic!("durably released turn must no longer have exact control");
    };
    assert_eq!(error.code, "stale_provider_turn");
    stop_and_release(&adapter, &active, &started).await;
}

#[tokio::test]
async fn post_io_panic_returns_driver_before_stopping_the_exact_runtime() {
    let _serial = super::tests::RUNTIME_TEST_LOCK.lock().await;
    let directory =
        tempfile::tempdir().unwrap_or_else(|error| panic!("create panic-custody fixture: {error}"));
    let session = fixture_session(directory.path(), "#!/bin/sh\nexit 0\n").await;
    let turn_entries = Arc::new(AtomicUsize::new(0));
    let stops = Arc::new(AtomicUsize::new(0));
    let adapter = ProviderAdapter::with_factory(Arc::new(PanicAfterIoFactory {
        turn_entries: Arc::clone(&turn_entries),
        stops: Arc::clone(&stops),
    }));
    let started = adapter
        .start(&session)
        .await
        .unwrap_or_else(|error| panic!("start panic-custody fixture: {error}"));
    let active = active_session(&session, &started, "panic-room-turn");
    let request = ProviderTurnRequest {
        turn_id: "panic-room-turn".to_owned(),
        turn_generation: 1,
        execution_id: "11111111-1111-4111-8111-111111111111".to_owned(),
        input: "Enter provider I/O, then panic.".to_owned(),
        room_observation: None,
    };

    let Err(error) = adapter.send_turn(&active, &request).await else {
        panic!("post-I/O panic must fail closed");
    };
    assert!(error.runtime_stopped);
    assert_eq!(error.runtime_lease_token, started.runtime_lease_token);
    assert_eq!(turn_entries.load(Ordering::SeqCst), 1);
    assert_eq!(stops.load(Ordering::SeqCst), 1);
    assert_eq!(
        adapter.observe(&active).await,
        ProviderRuntimeObservation::Gone
    );

    let Err(retry) = adapter.send_turn(&active, &request).await else {
        panic!("stopped runtime must not accept a duplicate turn");
    };
    assert_eq!(retry.code, "runtime_owner_mismatch");
    assert_eq!(turn_entries.load(Ordering::SeqCst), 1);
    adapter
        .release_confirmed_stop(
            &active.public.room_id,
            &active.public.session_id,
            &started.runtime_handle_id,
            &started.runtime_owner_id,
            &started.runtime_lease_token,
        )
        .await;
}

#[tokio::test]
async fn definitive_driver_failure_does_not_quarantine_or_discard_the_runtime() {
    let _serial = super::tests::RUNTIME_TEST_LOCK.lock().await;
    let directory = tempfile::tempdir()
        .unwrap_or_else(|error| panic!("create definitive-failure fixture: {error}"));
    let session = fixture_session(directory.path(), "#!/bin/sh\nexit 0\n").await;
    let adapter = ProviderAdapter::with_factory(Arc::new(DefinitiveFailureFactory));
    let started = adapter
        .start(&session)
        .await
        .unwrap_or_else(|error| panic!("start definitive-failure fixture: {error}"));
    let active = active_session(&session, &started, "definitive-room-turn");
    let request = ProviderTurnRequest {
        turn_id: "definitive-room-turn".to_owned(),
        turn_generation: 1,
        execution_id: "11111111-1111-4111-8111-111111111111".to_owned(),
        input: "Return one definitive provider failure.".to_owned(),
        room_observation: None,
    };

    let Err(error) = adapter.send_turn(&active, &request).await else {
        panic!("definitive provider failure must remain an error");
    };
    assert_eq!(error.code, "provider_credential_rejected");
    assert!(!error.effect_uncertain);
    assert!(!error.runtime_stopped);
    let reused = adapter
        .start(&active)
        .await
        .unwrap_or_else(|error| panic!("reuse runtime after definitive failure: {error}"));
    assert!(reused.runtime_reused);
    stop_and_release(&adapter, &active, &reused).await;
}

#[tokio::test]
async fn exact_control_freezes_provider_entry_before_durable_interrupt_wait() {
    let _serial = super::tests::RUNTIME_TEST_LOCK.lock().await;
    let directory = tempfile::tempdir()
        .unwrap_or_else(|error| panic!("create unstarted-turn fixture: {error}"));
    let transcript = directory.path().join("requests.jsonl");
    let script = turn_fixture(
        &transcript,
        "",
        "{\"jsonrpc\":\"2.0\",\"id\":3,\"result\":{\"turn\":{\"id\":\"provider-turn-1\"}}}",
        "",
    );
    let session = fixture_session(directory.path(), &script).await;
    let adapter = ProviderAdapter::new();
    let started = adapter
        .start(&session)
        .await
        .unwrap_or_else(|error| panic!("start unstarted-turn fixture: {error}"));
    let active = active_session(&session, &started, "room-turn-1");
    let request = ProviderTurnRequest {
        turn_id: "room-turn-1".to_owned(),
        turn_generation: 1,
        execution_id: "11111111-1111-4111-8111-111111111111".to_owned(),
        input: "Do not dispatch this turn.".to_owned(),
        room_observation: None,
    };
    let prepared = adapter
        .prepare_turn(&active, &request)
        .await
        .unwrap_or_else(|error| panic!("prepare unstarted turn: {error}"));
    let authority = prepared.exact_authority();
    let turn_gate = Arc::new(Barrier::new(2));
    let turn_adapter = adapter.clone();
    let turn_session = active.clone();
    let turn_request = request.clone();
    let turn_gate_owner = Arc::clone(&turn_gate);
    let turn = tokio::spawn(async move {
        turn_gate_owner.wait().await;
        turn_adapter
            .send_prepared_turn(prepared, &turn_session, &turn_request)
            .await
    });
    let mut control = adapter
        .begin_exact_turn(&authority)
        .await
        .unwrap_or_else(|error| panic!("capture unstarted turn: {error}"));
    assert_eq!(
        control.disposition,
        super::ProviderTurnInterruptDisposition::NotStarted
    );

    turn_gate.wait().await;
    let Err(error) = turn
        .await
        .unwrap_or_else(|error| panic!("join frozen turn owner: {error}"))
    else {
        panic!("exact control must freeze provider entry before persistence resumes");
    };
    assert_eq!(error.code, "provider_turn_interrupted");
    assert_eq!(
        control
            .wait_quiesced(Duration::from_secs(1))
            .await
            .unwrap_or_else(|error| panic!("prove unstarted turn: {error}")),
        super::ProviderTurnQuiescence::RuntimeRetained
    );
    assert!(adapter.owns_exact_turn(&authority).await);
    assert_eq!(adapter.retained_turn_result(&authority).await, None);
    assert_eq!(
        adapter
            .retained_not_started_proof(&authority)
            .await
            .as_ref()
            .map(super::ProviderTurnNotStartedProof::exact_authority),
        Some(&authority)
    );
    adapter.release_terminal_turn(&authority).await;
    assert!(!adapter.owns_exact_turn(&authority).await);
    assert!(
        requests(&transcript)
            .iter()
            .all(|request| { request["method"].as_str() != Some("turn/start") })
    );
    stop_and_release(&adapter, &active, &started).await;
}
