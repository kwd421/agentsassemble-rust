use crate::{
    ProviderCredentialError, ProviderCredentialId,
    credentials::{ProviderCredentialStore, private_handoff::SelectedCredential},
    provider_factory::{DriverFactory, ProductionDriverFactory},
    runtime_lease::{HeldRuntimeLease, LeaseObservation, observe_runtime_lease},
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[tokio::test]
async fn worker_entry() {
    if std::env::var_os("AGENTSASSEMBLE_TEST_MANAGED_WORKER").is_some() {
        std::process::exit(i32::from(super::platform::run_worker().await.is_err()));
    }
}

fn session()
-> Result<(agentsassemble_domain::DurableAgentSession, HeldRuntimeLease), std::io::Error> {
    let mut session = crate::test_support::durable_session(
        &uuid::Uuid::new_v4().to_string(),
        "windows-api",
        "DeepSeek",
        "deepseek_api",
        "deepseek-chat",
        "https",
    );
    "api".clone_into(&mut session.public.runtime_kind);
    let lease = HeldRuntimeLease::prepare(&session.public.room_id, &session.public.session_id)?;
    session.runtime_handle_id = lease.new_runtime_handle_id();
    session.runtime_lease_token = lease.token().to_owned();
    "windows-parent".clone_into(&mut session.runtime_owner_id);
    lease.begin_launch_effect()?;
    Ok((session, lease))
}

#[tokio::test]
async fn managed_api_uses_private_credentials_and_confirms_whole_job_stop() -> TestResult {
    for secret in [
        Err(ProviderCredentialError::SecureStoreUnavailable),
        Ok("synthetic-private-secret".to_owned()),
    ] {
        let available = secret.is_ok();
        let credentials = ProviderCredentialStore::from_private_handoff(Some(SelectedCredential {
            provider: ProviderCredentialId::DeepSeek,
            secret,
        }));
        let factory = ProductionDriverFactory::local(credentials);
        let (session, mut lease) = session()?;
        match factory.launch(&session, &lease).await {
            Ok(mut driver) => {
                assert!(available);
                assert!(!lease.cleanup_receipt_is_present());
                driver.attach_session(&session).await?;
                assert!(driver.is_alive().await?);
                driver.stop().await?;
            }
            Err(error) => {
                assert!(!available);
                assert_eq!(error.error.code, "secure_store_unavailable");
                assert!(!error.effect_uncertain);
            }
        }
        assert!(lease.cleanup_receipt_is_present());
        // A held lease remains exclusive until its supervisor releases the slot.
        assert_eq!(
            observe_runtime_lease(&session.public.room_id, &session.public.session_id),
            LeaseObservation::Active
        );
        lease.release_and_remove();
        assert_eq!(
            observe_runtime_lease(&session.public.room_id, &session.public.session_id),
            LeaseObservation::Missing
        );
    }
    Ok(())
}

#[tokio::test]
async fn cancelled_handshake_retains_real_job_cleanup_authority() -> TestResult {
    let factory = ProductionDriverFactory::local(ProviderCredentialStore::production());
    let (session, mut lease) = session()?;
    let spawned = super::platform::spawn(&factory, &session, &lease).await?;
    assert!(!lease.cleanup_receipt_is_present());
    // No launch frame or credential was sent. Dropping the launch future's owned
    // child must still terminate it; the retained lease observes the actual Job.
    let super::Spawn {
        child,
        connection,
        proof,
    } = spawned;
    drop(child);
    // Only this test retries a kernel observation: it does not drive cleanup.
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while !lease.cleanup_receipt_is_present() {
            tokio::task::yield_now().await;
        }
    })
    .await?;
    drop(connection);
    drop(proof);
    lease.release_and_remove();
    Ok(())
}
