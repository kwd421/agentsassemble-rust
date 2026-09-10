//! One attendee's admission, optional start, execution and positive cleanup lifecycle.
use super::{Advance, Custody, LocalAttendeeError};
use agentsassemble_domain::{
    LocalAttendeeCreate, LocalAttendeePhase as Phase, LocalAttendeeStatus,
};
use agentsassemble_persistence::SqliteStore;
use agentsassemble_provider::{ProviderAdapter, ProviderCatalogService};
use tokio::sync::{mpsc, oneshot, watch};
use tokio_util::sync::CancellationToken;

pub(super) struct Input {
    pub request: LocalAttendeeCreate,
    pub start: bool,
    pub catalog: ProviderCatalogService,
    pub store: SqliteStore,
    pub adapter: ProviderAdapter,
    pub status: watch::Sender<LocalAttendeeStatus>,
    pub commands: mpsc::Receiver<Advance>,
    pub cancellation: CancellationToken,
}

pub(super) async fn run(custody: &mut Custody, mut input: Input) -> Result<(), LocalAttendeeError> {
    if !admit(custody, &mut input).await? {
        return Ok(());
    }
    let execution = Box::pin(async {
        if input.cancellation.is_cancelled() {
            return Ok(None);
        }
        prepare(custody, &input).await?;
        if !input.start {
            publish(&input.status, Phase::Admitted, None);
            loop {
                tokio::select! {
                    () = input.cancellation.cancelled() => return Ok(None),
                    command = input.commands.recv() => match command {
                        Some(Advance::Start) => break,
                        // Concurrent admission retries may already be queued when recovery commits.
                        Some(Advance::RetryAdmission) => {},
                        None => return Err(LocalAttendeeError::new("local_attendee_action_unavailable")),
                    }
                }
            }
        }
        publish(&input.status, Phase::Starting, None);
        let joined = custody
            .joined
            .as_ref()
            .ok_or_else(|| LocalAttendeeError::new("local_attendee_admission_missing"))?;
        let runtime = custody
            .runtime
            .as_mut()
            .ok_or_else(|| LocalAttendeeError::new("local_attendee_runtime_missing"))?;
        let (ready, observation) = oneshot::channel();
        let session = crate::run_attendee_session(
            &custody.client,
            joined,
            runtime,
            &input.cancellation,
            Some(ready),
        );
        tokio::pin!(session);
        tokio::select! {
            result = &mut session => result.map_err(LocalAttendeeError::from),
            result = observation => {
                if result.is_ok() { publish(&input.status, Phase::Running, None); }
                session.await.map_err(LocalAttendeeError::from)
            }
        }
    })
    .await;
    publish(&input.status, Phase::Stopping, None);
    let (stop, failure) = match execution {
        Ok(stop) => (stop, None),
        Err(error) => (None, Some(error.code)),
    };
    crate::shutdown_attendee(&custody.client, custody.runtime.as_mut(), stop).await?;
    publish(
        &input.status,
        if failure.is_some() {
            Phase::Failed
        } else {
            Phase::Stopped
        },
        failure,
    );
    Ok(())
}

async fn admit(custody: &mut Custody, input: &mut Input) -> Result<bool, LocalAttendeeError> {
    if input.cancellation.is_cancelled() {
        publish(&input.status, Phase::Stopped, None);
        return Ok(false);
    }
    let mut uncertain = false;
    loop {
        publish(&input.status, Phase::Admitting, None);
        // Do not cancel an in-flight admission future: a lost response may follow commitment.
        match custody.client.join().await {
            Ok(joined) => {
                input.status.send_modify(|status| {
                    status.participant_id = Some(joined.participant_id.clone());
                });
                custody.joined = Some(joined);
                return Ok(true);
            }
            Err(error) if error.is_retryable() => {
                uncertain = true;
                publish(&input.status, Phase::AdmissionUnresolved, Some(error.code));
                tokio::select! {
                    () = input.cancellation.cancelled() => {
                        // A cancel resolves the same admission once before attempting its cleanup.
                        let joined = custody.client.join().await.map_err(|_| LocalAttendeeError::new("local_attendee_admission_unresolved"))?;
                        input.status.send_modify(|status| status.participant_id = Some(joined.participant_id.clone()));
                        custody.joined = Some(joined);
                        return Ok(true);
                    },
                    command = input.commands.recv() => {
                        if !matches!(command, Some(Advance::RetryAdmission)) {
                            return Err(LocalAttendeeError::new("local_attendee_admission_unresolved"));
                        }
                    }
                }
            }
            Err(error) => {
                if uncertain {
                    return Err(LocalAttendeeError::new(
                        "local_attendee_admission_unresolved",
                    ));
                }
                publish(&input.status, Phase::Failed, Some(error.code));
                return Ok(false);
            }
        }
    }
}

async fn prepare(custody: &mut Custody, input: &Input) -> Result<(), LocalAttendeeError> {
    let joined = custody
        .joined
        .as_ref()
        .ok_or_else(|| LocalAttendeeError::new("local_attendee_admission_missing"))?;
    let selection = input
        .catalog
        .validate_creation(
            &joined.room_id,
            &joined.participant_id,
            &input.request.request_id.to_string(),
            &input.request.creation,
        )
        .await
        .map_err(|error| LocalAttendeeError::new(error.code))?;
    let persona = if selection.persona_card_id.is_empty() {
        None
    } else {
        Some(
            input
                .store
                .persona_asset(&selection.persona_card_id)
                .await
                .map_err(|_| LocalAttendeeError::new("persona_asset_unavailable"))?,
        )
    };
    custody.runtime = Some(crate::AttendeeRuntime::new(
        joined,
        selection.into(),
        input.adapter.clone(),
        persona,
    )?);
    Ok(())
}

fn publish(status: &watch::Sender<LocalAttendeeStatus>, phase: Phase, error_code: Option<String>) {
    status.send_modify(|status| {
        status.phase = phase;
        status.error_code = error_code;
    });
}
