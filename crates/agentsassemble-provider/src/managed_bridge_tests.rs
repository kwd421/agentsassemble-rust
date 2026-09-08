use std::time::Duration;

use super::{
    run,
    wire::{self, Command, Event, Launch, read, write},
};
use crate::{
    ProviderCredentialError, ProviderCredentialId, ProviderCredentialStore,
    credentials::private_handoff::SelectedCredential,
    runtime::tests::{RUNTIME_TEST_LOCK, fixture_session},
    runtime_lease::{HeldRuntimeLease, LeaseObservation, observe_runtime_lease},
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[tokio::test]
async fn managed_private_pipe_preserves_native_attach_and_cleans_stop_loss_and_wrong_owner()
-> TestResult {
    let _serial = RUNTIME_TEST_LOCK.lock().await;
    for termination in ["stop", "eof", "wrong_owner"] {
        tokio::time::timeout(Duration::from_secs(20), native_lifecycle(termination)).await??;
    }
    Ok(())
}

async fn native_lifecycle(termination: &str) -> TestResult {
    let directory = tempfile::tempdir()?;
    let script = concat!(
        "#!/bin/sh\nIFS= read -r initialize\n",
        "printf '%s\\n' '{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}'\n",
        "IFS= read -r initialized\nIFS= read -r thread\n",
        "printf '%s\\n' '{\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"thread\":{\"id\":\"thread-1\"}}}'\n",
        "IFS= read -r forever\n",
    );
    let mut session = fixture_session(directory.path(), script).await;
    let mut lease = HeldRuntimeLease::prepare(&session.public.room_id, &session.public.session_id)?;
    session.runtime_handle_id = lease.new_runtime_handle_id();
    session.runtime_lease_token = lease.token().to_owned();
    session.runtime_owner_id = "fixture-parent".to_owned();
    lease.begin_launch_effect()?;
    let (parent, child) = tokio::io::duplex(8192);
    let (child_input, child_output) = tokio::io::split(child);
    let worker = tokio::spawn(run(child_input, child_output));
    let (parent_input, parent_output) = tokio::io::split(parent);
    let mut input = wire::reader(parent_input);
    let mut output = wire::writer(parent_output);
    write(
        &mut output,
        &Launch {
            session: Box::new(session.clone()),
            credential: None,
            state_root: None,
        },
    )
    .await?;
    assert!(matches!(
        read::<_, Event>(&mut input).await?,
        Some(Event::Ready { result: Ok(()) })
    ));
    lease.release_launch_lifetime();
    assert_eq!(
        observe_runtime_lease(&session.public.room_id, &session.public.session_id),
        LeaseObservation::Active
    );
    write(
        &mut output,
        &Command::Attach {
            id: 1,
            session: Box::new(session.clone()),
        },
    )
    .await?;
    assert!(
        matches!(read::<_, Event>(&mut input).await?, Some(Event::Attached { id: 1, result: Ok(attachment) }) if attachment.provider_session_id == "thread-1")
    );
    match termination {
        "stop" => {
            write(&mut output, &Command::Stop { id: 2 }).await?;
            assert!(matches!(
                read::<_, Event>(&mut input).await?,
                Some(Event::Stopped {
                    id: 2,
                    result: Ok(())
                })
            ));
        }
        "wrong_owner" => {
            session.runtime_owner_id = "different-parent".to_owned();
            write(
                &mut output,
                &Command::Attach {
                    id: 2,
                    session: Box::new(session.clone()),
                },
            )
            .await?;
        }
        _ => {}
    }
    drop(output);
    drop(input);
    let result = worker.await?;
    assert_eq!(result.is_ok(), termination == "stop");
    assert!(
        lease.cleanup_receipt_is_present(),
        "native process absence must be proven"
    );
    assert_eq!(
        observe_runtime_lease(&session.public.room_id, &session.public.session_id),
        LeaseObservation::GenerationGone {
            launch_token: lease.token().to_owned()
        }
    );
    lease.release_and_remove();
    Ok(())
}

#[tokio::test]
async fn private_credential_handoff_has_no_keyring_access_or_unselected_account() -> TestResult {
    let store = ProviderCredentialStore::from_private_handoff(Some(SelectedCredential {
        provider: ProviderCredentialId::DeepSeek,
        secret: Ok("synthetic-private-secret".to_owned()),
    }));
    assert_eq!(
        store.secret(ProviderCredentialId::DeepSeek).await?.expose(),
        "synthetic-private-secret"
    );
    assert!(matches!(
        store.secret(ProviderCredentialId::OpenRouter).await,
        Err(ProviderCredentialError::MissingSecret)
    ));
    assert_eq!(
        store.delete(ProviderCredentialId::DeepSeek).await,
        Err(ProviderCredentialError::SecureStoreUnavailable)
    );
    let failed = ProviderCredentialStore::from_private_handoff(Some(SelectedCredential {
        provider: ProviderCredentialId::DeepSeek,
        secret: Err(ProviderCredentialError::SecureStoreUnavailable),
    }));
    assert!(matches!(
        failed.secret(ProviderCredentialId::DeepSeek).await,
        Err(ProviderCredentialError::SecureStoreUnavailable)
    ));
    Ok(())
}
