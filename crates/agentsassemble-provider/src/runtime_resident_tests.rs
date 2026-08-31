use super::{
    ProviderAdapter, RuntimeState,
    tests::{RUNTIME_TEST_LOCK, code_mode_host_fixture},
};

#[tokio::test]
async fn resident_proof_requires_the_exact_available_live_driver() {
    let _serial = RUNTIME_TEST_LOCK.lock().await;
    let directory = tempfile::tempdir()
        .unwrap_or_else(|error| panic!("create resident proof fixture: {error}"));
    let (mut session, _arguments_report, _host_pid_report) =
        code_mode_host_fixture(directory.path()).await;
    let adapter = ProviderAdapter::new();
    let started = adapter
        .start(&session)
        .await
        .unwrap_or_else(|error| panic!("start resident proof fixture: {error}"));
    session
        .runtime_handle_id
        .clone_from(&started.runtime_handle_id);
    session
        .runtime_owner_id
        .clone_from(&started.runtime_owner_id);
    session
        .runtime_lease_token
        .clone_from(&started.runtime_lease_token);
    assert!(adapter.prove_resident_runtime(&session).await.is_ok());

    let driver = {
        let slot = adapter
            .existing_slot(&session.public.room_id, &session.public.session_id)
            .await
            .unwrap_or_else(|| panic!("resident proof slot is unavailable"));
        let mut slot = slot.lock().await;
        let RuntimeState::Running(runtime) = &mut slot.state else {
            panic!("resident proof runtime is not running");
        };
        runtime
            .driver
            .try_take()
            .unwrap_or_else(|error| panic!("take resident proof driver: {error}"))
    };
    let Err(unavailable) = adapter.prove_resident_runtime(&session).await else {
        panic!("a borrowed driver cannot prove an idle resident runtime");
    };
    assert_eq!(unavailable.code, "provider_turn_active");
    let slot = adapter
        .existing_slot(&session.public.room_id, &session.public.session_id)
        .await
        .unwrap_or_else(|| panic!("resident proof slot disappeared"));
    let cell = {
        let mut slot = slot.lock().await;
        let RuntimeState::Running(runtime) = &mut slot.state else {
            panic!("resident proof runtime changed state");
        };
        runtime.driver.clone()
    };
    cell.put(driver).await;

    adapter
        .stop(
            &session.public.room_id,
            &session.public.session_id,
            &started.runtime_handle_id,
            &started.runtime_owner_id,
            &started.runtime_lease_token,
        )
        .await
        .unwrap_or_else(|error| panic!("stop resident proof fixture: {error}"));
    let Err(stopped) = adapter.prove_resident_runtime(&session).await else {
        panic!("a stopped runtime cannot prove residency");
    };
    assert_eq!(stopped.code, "resident_runtime_unavailable");
    adapter
        .release_confirmed_stop(
            &session.public.room_id,
            &session.public.session_id,
            &started.runtime_handle_id,
            &started.runtime_owner_id,
            &started.runtime_lease_token,
        )
        .await;
}
