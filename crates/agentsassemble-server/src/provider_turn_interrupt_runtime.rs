//! Executes and reconciles one persistence-owned exact provider-turn interrupt.

use std::time::Duration;

use agentsassemble_persistence::{
    AgentTurnCommit, PersistenceError, ProviderTurnInterruptEffect, SqliteStore,
};
use agentsassemble_provider::{
    ProviderAdapter, ProviderExactTurnAuthority, ProviderTurnInterruptDisposition,
    ProviderTurnQuiescence,
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
                .handoff_unissued_provider_interrupt_claim(&claim)
                .await?;
            return Err(unresolved(error.code, error.message));
        }
    };
    let waiting = match control.disposition {
        ProviderTurnInterruptDisposition::NotStarted
        | ProviderTurnInterruptDisposition::Quiesced => {
            store.mark_unstarted_interrupt_waiting(&claim).await?
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
    };
    let quiescence = match control.wait_quiesced(QUIESCENCE_TIMEOUT).await {
        Ok(quiescence) => quiescence,
        Err(error) => {
            store
                .mark_provider_interrupt_recovery_required(&waiting)
                .await?;
            return Err(unresolved(error.code, error.message));
        }
    };
    finalize_exact_quiescence(store, provider_adapter, &authority, &waiting, quiescence).await
}

pub(crate) async fn resume_exact_interrupt(
    store: &SqliteStore,
    provider_adapter: &ProviderAdapter,
    effect: &ProviderTurnInterruptEffect,
) -> Result<Option<AgentTurnCommit>, PersistenceError> {
    let claim = match store
        .claim_provider_turn_interrupt_recovery(effect, &Uuid::new_v4().to_string())
        .await
    {
        Ok(claim) => claim,
        Err(PersistenceError::CommandUnresolved {
            code: "provider_turn_effect_unresolved",
            ..
        }) => return Ok(None),
        Err(error) => return Err(error),
    };
    let authority = exact_authority(&claim.effect);
    let Ok(mut control) = provider_adapter.begin_exact_turn(&authority).await else {
        store
            .release_provider_interrupt_recovery_claim(&claim)
            .await?;
        return Ok(None);
    };
    let waiting = store
        .authorize_provider_interrupt_recovery_wait(&claim)
        .await?;
    if control.disposition != ProviderTurnInterruptDisposition::Quiesced {
        control.request_interrupt();
    }
    let quiescence = match control.wait_quiesced(QUIESCENCE_TIMEOUT).await {
        Ok(quiescence) => quiescence,
        Err(error) => {
            store
                .mark_provider_interrupt_recovery_required(&waiting)
                .await?;
            return Err(unresolved(error.code, error.message));
        }
    };
    finalize_exact_quiescence(store, provider_adapter, &authority, &waiting, quiescence)
        .await
        .map(Some)
}

async fn finalize_exact_quiescence(
    store: &SqliteStore,
    provider_adapter: &ProviderAdapter,
    authority: &ProviderExactTurnAuthority,
    effect: &ProviderTurnInterruptEffect,
    quiescence: ProviderTurnQuiescence,
) -> Result<AgentTurnCommit, PersistenceError> {
    match quiescence {
        ProviderTurnQuiescence::RuntimeRetained => {
            let commit = store.finalize_interrupted_turn_retained(effect).await?;
            provider_adapter.release_terminal_turn(authority).await;
            Ok(commit)
        }
        ProviderTurnQuiescence::RuntimeGone => {
            let candidate = store
                .load_provider_turn_reconciliation_candidate(
                    &effect.room_id,
                    &effect.session_id,
                    effect.turn_generation,
                )
                .await?;
            if candidate.effect.as_ref() != Some(effect) {
                return Err(unresolved(
                    "provider_turn_interrupt_changed",
                    "The exact provider interrupt changed before runtime-gone finalization.",
                ));
            }
            let commit = store
                .finalize_provider_turn_runtime_gone(&candidate)
                .await?;
            provider_adapter
                .release_confirmed_stop(
                    &effect.room_id,
                    &effect.session_id,
                    &effect.runtime_handle_id,
                    &effect.runtime_owner_id,
                    &effect.runtime_lease_token,
                )
                .await;
            Ok(commit)
        }
    }
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

#[cfg(test)]
#[path = "provider_turn_interrupt_runtime_tests.rs"]
mod tests;
