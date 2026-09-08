use std::time::Duration;

use super::wire::{self, Command, Event, Launch, read, write};
use crate::{
    ProviderCredentialError, ProviderCredentialId, ProviderCredentialStore,
    credentials::private_handoff::SelectedCredential,
    runtime::tests::{RUNTIME_TEST_LOCK, fixture_session},
    runtime_lease::{HeldRuntimeLease, LeaseObservation, observe_runtime_lease},
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[tokio::test]
async fn managed_api_preserves_private_credential_failure_and_worker_custody() -> TestResult {
    for secret in [
        Err(ProviderCredentialError::SecureStoreUnavailable),
        Ok("synthetic-private-secret".to_owned()),
    ] {
        managed_api_lifecycle(secret).await?;
    }
    Ok(())
}

async fn managed_api_lifecycle(secret: Result<String, ProviderCredentialError>) -> TestResult {
    use crate::provider_factory::DriverFactory;
    let available = secret.is_ok();
    let mut session = crate::test_support::durable_session(
        &uuid::Uuid::new_v4().to_string(),
        "api-worker",
        "DeepSeek",
        "deepseek_api",
        "deepseek-chat",
        "https",
    );
    "api".clone_into(&mut session.public.runtime_kind);
    let credentials = ProviderCredentialStore::from_private_handoff(Some(SelectedCredential {
        provider: ProviderCredentialId::DeepSeek,
        secret,
    }));
    let factory = crate::provider_factory::ProductionDriverFactory::local(credentials);
    let mut lease = HeldRuntimeLease::prepare(&session.public.room_id, &session.public.session_id)?;
    session.runtime_handle_id = lease.new_runtime_handle_id();
    session.runtime_lease_token = lease.token().to_owned();
    "api-parent".clone_into(&mut session.runtime_owner_id);
    lease.begin_launch_effect()?;
    match factory.launch(&session, &lease).await {
        Ok(mut driver) => {
            assert!(available);
            assert_eq!(
                observe_runtime_lease(&session.public.room_id, &session.public.session_id),
                LeaseObservation::Active
            );
            // Native registration consumes the selected secret at startup. No HTTP turn runs.
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
async fn managed_private_pipe_preserves_native_attach_and_cleans_stop_loss_and_wrong_owner()
-> TestResult {
    let _serial = RUNTIME_TEST_LOCK.lock().await;
    for termination in ["stop", "eof", "wrong_owner", "answer", "interrupt", "plain"] {
        tokio::time::timeout(Duration::from_secs(20), native_lifecycle(termination))
            .await
            .map_err(|_| format!("{termination}: worker deadline"))?
            .map_err(|error| format!("{termination}: {error}"))?;
    }
    Ok(())
}

async fn native_lifecycle(termination: &str) -> TestResult {
    let directory = tempfile::tempdir()?;
    let script = idle_script();
    let transcript = directory.path().join("native.jsonl");
    let turn_case = matches!(termination, "answer" | "interrupt" | "plain");
    let script = if turn_case {
        crate::runtime::codex_request_tests::request_fixture(
            &transcript,
            termination == "interrupt",
        )
    } else {
        script.to_owned()
    };
    let mut session = fixture_session(directory.path(), &script).await;
    let mut lease = HeldRuntimeLease::prepare(&session.public.room_id, &session.public.session_id)?;
    session.runtime_handle_id = lease.new_runtime_handle_id();
    session.runtime_lease_token = lease.token().to_owned();
    "fixture-parent".clone_into(&mut session.runtime_owner_id);
    lease.begin_launch_effect()?;
    let (_binding, mut worker, parent) = super::process_tests::spawn_worker()?;
    let (parent_input, parent_output) = parent.into_split();
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
        read_event(&mut input).await?,
        Some(Event::Acquired)
    ));
    lease.release_launch_lifetime();
    assert!(matches!(
        read_event(&mut input).await?,
        Some(Event::Ready { result: Ok(()) })
    ));
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
        matches!(read_event(&mut input).await?, Some(Event::Attached { id: 1, result: Ok(attachment) }) if attachment.provider_session_id == "thread-1")
    );
    let next_id = if turn_case {
        "thread-1".clone_into(&mut session.provider_session_id);
        native_request_turn(
            &mut input,
            &mut output,
            &session,
            termination == "interrupt",
            termination == "plain",
        )
        .await?
    } else {
        2
    };
    match termination {
        "stop" | "answer" | "interrupt" | "plain" => {
            write(&mut output, &Command::Stop { id: next_id }).await?;
            assert!(matches!(
                read_event(&mut input).await?,
                Some(Event::Stopped { result: Ok(()), .. })
            ));
        }
        "wrong_owner" => {
            "different-parent".clone_into(&mut session.runtime_owner_id);
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
    let result = if worker.wait().await?.success() {
        Ok(())
    } else {
        Err(super::protocol_error())
    };
    assert_native_proof(&mut lease, &session, &result, &transcript, termination)
}

fn assert_native_proof(
    lease: &mut HeldRuntimeLease,
    session: &agentsassemble_domain::DurableAgentSession,
    result: &Result<(), crate::driver::DriverError>,
    transcript: &std::path::Path,
    termination: &str,
) -> TestResult {
    assert_eq!(
        result.is_ok(),
        matches!(termination, "stop" | "answer" | "interrupt" | "plain")
    );
    if matches!(termination, "answer" | "interrupt" | "plain") {
        let recorded = std::fs::read_to_string(transcript)?;
        let frames = recorded
            .lines()
            .map(serde_json::from_str::<serde_json::Value>)
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(
            frames.len(),
            5,
            "exactly one native send and one answer or interrupt"
        );
        assert_eq!(frames[3]["method"], "turn/start");
        if termination == "interrupt" {
            assert_eq!(frames[4]["method"], "turn/interrupt");
        } else {
            assert_eq!(frames[4]["id"], "native-request-1");
        }
    }
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

async fn read_event<R: tokio::io::AsyncRead + Unpin>(
    input: &mut wire::Reader<R>,
) -> Result<Option<Event>, crate::driver::DriverError> {
    loop {
        match read(input).await? {
            Some(Event::Facts { .. }) => {}
            event => return Ok(event),
        }
    }
}

async fn native_request_turn<R: tokio::io::AsyncRead + Unpin, W: tokio::io::AsyncWrite + Unpin>(
    input: &mut wire::Reader<R>,
    output: &mut wire::Writer<W>,
    session: &agentsassemble_domain::DurableAgentSession,
    interrupt: bool,
    plain: bool,
) -> Result<u64, Box<dyn std::error::Error>> {
    use super::callbacks::{Callback, Reply};
    write(output, &prepare_turn(session, plain)?).await?;
    assert!(matches!(
        read_event(input).await?,
        Some(Event::Prepared {
            id: 2,
            result: Ok(())
        })
    ));
    write(
        output,
        &Command::Send {
            id: 3,
            session: Box::new(session.clone()),
        },
    )
    .await?;
    let Some(Event::Callback {
        id,
        callback: Callback::Request { .. },
    }) = read_event(input).await?
    else {
        return Err("expected native request".into());
    };
    write(
        output,
        &Command::Callback {
            id: 4,
            callback_id: id,
            reply: Reply::RequestOpened { result: Ok(()) },
        },
    )
    .await?;
    let early_delivery = if interrupt {
        write(
            output,
            &Command::Interrupt {
                id: 5,
                session: Box::new(session.clone()),
            },
        )
        .await?;
        assert!(matches!(
            read_event(input).await?,
            Some(Event::TurnCancelled { id: 3 })
        ));
        await_interruption(input).await?
    } else {
        write(
            output,
            &Command::Callback {
                id: 5,
                callback_id: id,
                reply: Reply::Resolution {
                    result: Ok(agentsassemble_domain::ProviderRequestResolution::Answers {
                        answers: [("answer".to_owned(), vec!["fixture-value".to_owned()])].into(),
                    }),
                },
            },
        )
        .await?;
        None
    };
    let (delivered_id, delivered) = if let Some(delivery) = early_delivery {
        delivery
    } else {
        let Some(Event::Callback {
            id,
            callback: Callback::Delivered { delivered },
        }) = read_event(input).await?
        else {
            return Err("expected native delivery acknowledgement".into());
        };
        (id, delivered)
    };
    assert_eq!(delivered_id, id);
    assert_eq!(delivered, !interrupt);
    write(
        output,
        &Command::Callback {
            id: 6,
            callback_id: id,
            reply: Reply::Receipt { result: Ok(()) },
        },
    )
    .await?;
    if !interrupt {
        assert!(matches!(
            read_event(input).await?,
            Some(Event::Turn {
                id: 3,
                result: Ok(_)
            })
        ));
    }
    close_turn(input, output, session, plain).await
}

async fn close_turn<R: tokio::io::AsyncRead + Unpin, W: tokio::io::AsyncWrite + Unpin>(
    input: &mut wire::Reader<R>,
    output: &mut wire::Writer<W>,
    session: &agentsassemble_domain::DurableAgentSession,
    plain: bool,
) -> Result<u64, Box<dyn std::error::Error>> {
    let abort_id = if plain {
        write(
            output,
            &Command::Interrupt {
                id: 7,
                session: Box::new(session.clone()),
            },
        )
        .await?;
        assert!(matches!(
            read_event(input).await?,
            Some(Event::Interrupted {
                id: 7,
                result: Ok(())
            })
        ));
        8
    } else {
        7
    };
    write(output, &Command::Abort { id: abort_id }).await?;
    assert!(matches!(
        read_event(input).await?,
        Some(Event::Aborted { result: Ok(()), .. })
    ));
    Ok(abort_id + 1)
}

fn prepare_turn(
    session: &agentsassemble_domain::DurableAgentSession,
    plain: bool,
) -> Result<Command, serde_json::Error> {
    let mut turn: super::turn::TurnInput = serde_json::from_value(serde_json::json!({
        "turn_id": "room-turn-1", "turn_generation": 1,
        "execution_id": "11111111-1111-4111-8111-111111111111",
        "input": "Ask a question", "requests": true,
        "observation": { "session_id": session.public.session_id, "input_up_to_seq": 9,
            "view": "Room: General\n#9 Human: ask", "attachment_ids": [],
            "attachments": false, "allowed_agent_ids": [], "tabletop_tools": false, "tools": false }
    }))?;
    if plain {
        turn.observation = None;
    }
    Ok(Command::Prepare { id: 2, turn })
}

async fn await_interruption<R: tokio::io::AsyncRead + Unpin>(
    input: &mut wire::Reader<R>,
) -> Result<Option<(u64, bool)>, Box<dyn std::error::Error>> {
    let mut delivery = None;
    loop {
        match read_event(input).await? {
            Some(Event::Interrupted {
                id: 5,
                result: Ok(()),
            }) => return Ok(delivery),
            Some(Event::Callback {
                id,
                callback: super::callbacks::Callback::Delivered { delivered },
            }) if delivery.is_none() => delivery = Some((id, delivered)),
            _ => return Err("unexpected interrupt or delivery response".into()),
        }
    }
}

fn idle_script() -> &'static str {
    concat!(
        "#!/bin/sh\nIFS= read -r initialize\n",
        "printf '%s\\n' '{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}'\n",
        "IFS= read -r initialized\nIFS= read -r thread\n",
        "printf '%s\\n' '{\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"thread\":{\"id\":\"thread-1\"}}}'\n",
        "IFS= read -r forever\n",
    )
}
