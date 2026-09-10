use super::{
    ProviderAdapter, RuntimeState,
    tests::{RUNTIME_TEST_LOCK, code_mode_host_fixture},
};

#[tokio::test]
async fn shutdown_cancels_all_owned_runtimes_before_waiting_for_any_driver()
-> Result<(), Box<dyn std::error::Error>> {
    let _serial = RUNTIME_TEST_LOCK.lock().await;
    let first = tempfile::tempdir()?;
    let second = tempfile::tempdir()?;
    let adapter = ProviderAdapter::new();
    let mut held = Vec::new();
    for directory in [&first, &second] {
        let (session, _, _) = code_mode_host_fixture(directory.path()).await;
        adapter.start(&session).await?;
        let slot = adapter
            .existing_slot(&session.public.room_id, &session.public.session_id)
            .await
            .ok_or("missing started runtime")?;
        let mut slot = slot.lock().await;
        let RuntimeState::Running(runtime) = &mut slot.state else {
            return Err("expected running runtime".into());
        };
        let driver = runtime.driver.try_take()?;
        held.push((
            runtime.driver.clone(),
            driver,
            runtime.turn_cancellation.clone(),
        ));
    }
    let owner = adapter.clone();
    let shutdown = tokio::spawn(async move { owner.shutdown_with_observations().await });
    // Neither driver is returned until both independent owners receive stop. A
    // serial drain instead consumes its five-second driver timeout on the first.
    let cancelled = tokio::time::timeout(std::time::Duration::from_secs(1), async {
        for (_, _, cancellation) in &held {
            cancellation.cancelled().await;
        }
    })
    .await;
    for (cell, driver, _) in held {
        cell.put(driver).await;
    }
    let outcome = shutdown.await?;
    adapter.release_shutdown_observations(&outcome.gone).await;
    assert!(
        cancelled.is_ok(),
        "all owners must be cancelled before any driver wait"
    );
    assert!(outcome.failure.is_none());
    assert_eq!(outcome.gone.len(), 2);
    assert!(adapter.shutdown_with_observations().await.gone.is_empty());
    Ok(())
}

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
