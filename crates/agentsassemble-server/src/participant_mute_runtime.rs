use std::time::Duration;

use agentsassemble_persistence::{
    AgentTurnCommit, PersistenceError, ProviderTurnInterruptEffect, SqliteStore,
};
use agentsassemble_provider::{
    ProviderAdapter, ProviderExactTurnAuthority, ProviderTurnInterruptDisposition,
};
use uuid::Uuid;

const QUIESCENCE_TIMEOUT: Duration = Duration::from_secs(15);

pub(crate) async fn apply_exact_interrupt(
    store: &SqliteStore,
    provider_adapter: &ProviderAdapter,
    effect: &ProviderTurnInterruptEffect,
) -> Result<AgentTurnCommit, PersistenceError> {
    let claim = store
        .claim_provider_turn_interrupt(effect, &Uuid::new_v4().to_string())
        .await?;
    let authority = exact_authority(&claim.effect);
    let mut control = match provider_adapter.begin_exact_turn(&authority).await {
        Ok(control) => control,
        Err(error) => {
            store
                .mark_provider_interrupt_recovery_required(&claim.effect)
                .await?;
            return Err(unresolved(error.code, error.message));
        }
    };
    let waiting = match control.disposition {
        ProviderTurnInterruptDisposition::NotStarted => {
            let waiting = store.mark_unstarted_interrupt_waiting(&claim).await?;
            control.request_interrupt();
            waiting
        }
        ProviderTurnInterruptDisposition::Started => {
            let dispatched = store.authorize_provider_interrupt_dispatch(&claim).await?;
            control.request_interrupt();
            match store.mark_provider_interrupt_issued(&dispatched).await {
                Ok(waiting) => waiting,
                Err(error) => {
                    store.mark_provider_interrupt_ambiguous(&dispatched).await?;
                    return Err(error);
                }
            }
        }
        ProviderTurnInterruptDisposition::Quiesced => {
            store.mark_unstarted_interrupt_waiting(&claim).await?
        }
    };
    if let Err(error) = control.wait_quiesced(QUIESCENCE_TIMEOUT).await {
        store
            .mark_provider_interrupt_recovery_required(&waiting)
            .await?;
        return Err(unresolved(error.code, error.message));
    }
    let commit = store.finalize_interrupted_turn_retained(&waiting).await?;
    provider_adapter.release_terminal_turn(&authority).await;
    Ok(commit)
}

pub(crate) async fn resume_exact_interrupt(
    store: &SqliteStore,
    provider_adapter: &ProviderAdapter,
    effect: &ProviderTurnInterruptEffect,
) -> Result<Option<AgentTurnCommit>, PersistenceError> {
    let authority = exact_authority(effect);
    let Ok(mut control) = provider_adapter.begin_exact_turn(&authority).await else {
        return Ok(None);
    };
    let waiting = store
        .authorize_provider_interrupt_recovery_wait(effect)
        .await?;
    if control.disposition != ProviderTurnInterruptDisposition::Quiesced {
        control.request_interrupt();
    }
    if let Err(error) = control.wait_quiesced(QUIESCENCE_TIMEOUT).await {
        store
            .mark_provider_interrupt_recovery_required(&waiting)
            .await?;
        return Err(unresolved(error.code, error.message));
    }
    let commit = store.finalize_interrupted_turn_retained(&waiting).await?;
    provider_adapter.release_terminal_turn(&authority).await;
    Ok(Some(commit))
}

fn exact_authority(effect: &ProviderTurnInterruptEffect) -> ProviderExactTurnAuthority {
    ProviderExactTurnAuthority {
        room_id: effect.room_id.clone(),
        session_id: effect.session_id.clone(),
        execution_id: effect.execution_id.clone(),
        turn_id: effect.turn_id.clone(),
        turn_generation: effect.turn_generation,
        runtime_handle_id: effect.runtime_handle_id.clone(),
        runtime_owner_id: effect.runtime_owner_id.clone(),
        runtime_lease_token: effect.runtime_lease_token.clone(),
    }
}

fn unresolved(code: &'static str, message: &'static str) -> PersistenceError {
    PersistenceError::CommandUnresolved {
        code,
        message: message.to_owned(),
    }
}
