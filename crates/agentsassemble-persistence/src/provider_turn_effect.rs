use chrono::{SecondsFormat, Utc};
use sqlx::{Row, Sqlite, Transaction};
use uuid::Uuid;

use crate::{
    PersistenceError, ProviderTurnExecution, ProviderTurnExecutionPhase, SqliteStore,
    provider_turn_execution::load_execution_in,
};

const CLAIM_TTL_MILLIS: i64 = 30_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderTurnEffectPhase {
    Prepared,
    Claimed,
    Dispatching,
    IssuedWaitingQuiescence,
    InterruptAmbiguous,
    RecoveryRequired,
    Finalized,
}

impl ProviderTurnEffectPhase {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Prepared => "prepared",
            Self::Claimed => "claimed",
            Self::Dispatching => "dispatching",
            Self::IssuedWaitingQuiescence => "issued_waiting_quiescence",
            Self::InterruptAmbiguous => "interrupt_ambiguous",
            Self::RecoveryRequired => "recovery_required",
            Self::Finalized => "finalized",
        }
    }

    fn parse(value: &str) -> Result<Self, PersistenceError> {
        match value {
            "prepared" => Ok(Self::Prepared),
            "claimed" => Ok(Self::Claimed),
            "dispatching" => Ok(Self::Dispatching),
            "issued_waiting_quiescence" => Ok(Self::IssuedWaitingQuiescence),
            "interrupt_ambiguous" => Ok(Self::InterruptAmbiguous),
            "recovery_required" => Ok(Self::RecoveryRequired),
            "finalized" => Ok(Self::Finalized),
            _ => Err(invalid_effect()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderTurnInterruptEffect {
    pub room_id: String,
    pub session_id: String,
    pub turn_generation: u64,
    pub effect_id: String,
    pub phase: ProviderTurnEffectPhase,
    pub execution_id: String,
    pub participant_id: String,
    pub turn_id: String,
    pub start_dispatch_nonce: String,
    pub runtime_handle_id: String,
    pub runtime_owner_id: String,
    pub runtime_lease_token: String,
    pub dispatch_nonce: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderTurnEffectClaim {
    pub effect: ProviderTurnInterruptEffect,
    pub claim_owner: String,
}

pub(crate) async fn prepare_interrupt_effect(
    transaction: &mut Transaction<'_, Sqlite>,
    execution: &ProviderTurnExecution,
) -> Result<ProviderTurnInterruptEffect, PersistenceError> {
    if !execution.phase.is_blocking() {
        return Err(stale_effect());
    }
    let effect_id = Uuid::new_v4().to_string();
    let now = canonical_now();
    sqlx::query(
        "INSERT OR IGNORE INTO provider_turn_effects(\
         room_id, session_id, turn_generation, effect_id, effect_kind, phase, created_at, updated_at) \
         VALUES (?, ?, ?, ?, 'interrupt', 'prepared', ?, ?)",
    )
    .bind(&execution.room_id)
    .bind(&execution.session_id)
    .bind(generation_i64(execution.turn_generation)?)
    .bind(effect_id)
    .bind(&now)
    .bind(&now)
    .execute(&mut **transaction)
    .await?;
    if matches!(
        execution.phase,
        ProviderTurnExecutionPhase::Assigned
            | ProviderTurnExecutionPhase::StartDispatching
            | ProviderTurnExecutionPhase::Running
    ) {
        let changed = sqlx::query(
            "UPDATE provider_turn_executions SET phase = 'interrupt_pending', updated_at = ? \
             WHERE room_id = ? AND session_id = ? AND turn_generation = ? \
             AND execution_id = ? AND turn_id = ? AND phase = ? \
             AND runtime_handle_id = ? AND runtime_owner_id = ? AND runtime_lease_token = ?",
        )
        .bind(&now)
        .bind(&execution.room_id)
        .bind(&execution.session_id)
        .bind(generation_i64(execution.turn_generation)?)
        .bind(&execution.execution_id)
        .bind(&execution.turn_id)
        .bind(execution.phase.as_str())
        .bind(&execution.runtime_handle_id)
        .bind(&execution.runtime_owner_id)
        .bind(&execution.runtime_lease_token)
        .execute(&mut **transaction)
        .await?;
        if changed.rows_affected() != 1 {
            return Err(stale_effect());
        }
    }
    load_effect_in(
        transaction,
        &execution.room_id,
        &execution.session_id,
        execution.turn_generation,
    )
    .await
}

impl SqliteStore {
    /// Claims a prepared exact-turn interrupt without issuing provider I/O.
    ///
    /// # Errors
    ///
    /// Rejects stale custody, an unresolved dispatched effect, or another live claimant.
    pub async fn claim_provider_turn_interrupt(
        &self,
        expected: &ProviderTurnInterruptEffect,
        claim_owner: &str,
    ) -> Result<ProviderTurnEffectClaim, PersistenceError> {
        validate_claim_owner(claim_owner)?;
        let mut transaction = self.pool.begin().await?;
        let current = load_effect_in(
            &mut transaction,
            &expected.room_id,
            &expected.session_id,
            expected.turn_generation,
        )
        .await?;
        require_exact_effect(expected, &current)?;
        let now_millis = Utc::now().timestamp_millis();
        let expires_at = now_millis.saturating_add(CLAIM_TTL_MILLIS);
        let changed = sqlx::query(
            "UPDATE provider_turn_effects SET phase = 'claimed', claim_owner = ?, \
             claim_expires_at = ?, updated_at = ? WHERE room_id = ? AND effect_id = ? \
             AND (phase = 'prepared' OR (phase = 'claimed' \
               AND (claim_owner = ? OR claim_expires_at < ?)))",
        )
        .bind(claim_owner)
        .bind(expires_at)
        .bind(canonical_now())
        .bind(&current.room_id)
        .bind(&current.effect_id)
        .bind(claim_owner)
        .bind(now_millis)
        .execute(&mut *transaction)
        .await?;
        if changed.rows_affected() != 1 {
            return Err(unresolved_effect());
        }
        let effect = load_effect_in(
            &mut transaction,
            &current.room_id,
            &current.session_id,
            current.turn_generation,
        )
        .await?;
        transaction.commit().await?;
        Ok(ProviderTurnEffectClaim {
            effect,
            claim_owner: claim_owner.to_owned(),
        })
    }

    /// Leases one unresolved exact-turn interrupt before touching live provider control.
    ///
    /// # Errors
    ///
    /// Rejects stale custody, a terminal pair, or another unexpired recovery claimant.
    pub async fn claim_provider_turn_interrupt_recovery(
        &self,
        expected: &ProviderTurnInterruptEffect,
        claim_owner: &str,
    ) -> Result<ProviderTurnEffectClaim, PersistenceError> {
        validate_claim_owner(claim_owner)?;
        let mut transaction = self.pool.begin().await?;
        let current = load_effect_in(
            &mut transaction,
            &expected.room_id,
            &expected.session_id,
            expected.turn_generation,
        )
        .await?;
        require_exact_effect(expected, &current)?;
        let execution = load_execution_in(
            &mut transaction,
            &expected.room_id,
            &expected.session_id,
            expected.turn_generation,
        )
        .await?;
        if !recoverable_interrupt_pair(current.phase, execution.phase)
            || execution.execution_id != current.execution_id
            || execution.runtime_handle_id != current.runtime_handle_id
            || execution.runtime_owner_id != current.runtime_owner_id
            || execution.runtime_lease_token != current.runtime_lease_token
            || execution.requeue_finalized
        {
            return Err(stale_effect());
        }
        let now_millis = Utc::now().timestamp_millis();
        let expires_at = now_millis.saturating_add(CLAIM_TTL_MILLIS);
        let changed = sqlx::query(
            "UPDATE provider_turn_effects SET claim_owner = ?, claim_expires_at = ?, \
             updated_at = ? WHERE room_id = ? AND effect_id = ? AND phase = ? \
             AND dispatch_nonce = ? AND (claim_owner = '' OR claim_owner = ? \
               OR claim_expires_at IS NULL OR claim_expires_at < ?)",
        )
        .bind(claim_owner)
        .bind(expires_at)
        .bind(canonical_now())
        .bind(&current.room_id)
        .bind(&current.effect_id)
        .bind(current.phase.as_str())
        .bind(&current.dispatch_nonce)
        .bind(claim_owner)
        .bind(now_millis)
        .execute(&mut *transaction)
        .await?;
        if changed.rows_affected() != 1 {
            return Err(unresolved_effect());
        }
        let effect = load_effect_in(
            &mut transaction,
            &current.room_id,
            &current.session_id,
            current.turn_generation,
        )
        .await?;
        transaction.commit().await?;
        Ok(ProviderTurnEffectClaim {
            effect,
            claim_owner: claim_owner.to_owned(),
        })
    }

    /// Records quiescence waiting for a turn proved not to have started externally.
    ///
    /// # Errors
    ///
    /// Rejects a stale or no-longer-claimed exact effect.
    pub async fn mark_unstarted_interrupt_waiting(
        &self,
        claim: &ProviderTurnEffectClaim,
    ) -> Result<ProviderTurnInterruptEffect, PersistenceError> {
        transition_to_waiting(self, claim, false).await
    }

    /// Durably fences the first provider interrupt byte or request.
    ///
    /// # Errors
    ///
    /// Rejects a stale claim or effect that already crossed dispatch.
    pub async fn authorize_provider_interrupt_dispatch(
        &self,
        claim: &ProviderTurnEffectClaim,
    ) -> Result<ProviderTurnInterruptEffect, PersistenceError> {
        let dispatch_nonce = Uuid::new_v4().to_string();
        let now_millis = Utc::now().timestamp_millis();
        let changed = sqlx::query(
            "UPDATE provider_turn_effects SET phase = 'dispatching', dispatch_nonce = ?, \
             updated_at = ? WHERE room_id = ? AND effect_id = ? AND phase = 'claimed' \
             AND claim_owner = ? AND claim_expires_at >= ?",
        )
        .bind(&dispatch_nonce)
        .bind(canonical_now())
        .bind(&claim.effect.room_id)
        .bind(&claim.effect.effect_id)
        .bind(&claim.claim_owner)
        .bind(now_millis)
        .execute(&self.pool)
        .await?;
        if changed.rows_affected() != 1 {
            return Err(stale_effect());
        }
        self.provider_turn_interrupt_effect(
            &claim.effect.room_id,
            &claim.effect.session_id,
            claim.effect.turn_generation,
        )
        .await
    }

    /// Records a confirmed interrupt dispatch and begins exact quiescence waiting.
    ///
    /// # Errors
    ///
    /// Rejects changed dispatch, execution, or runtime custody authority.
    pub async fn mark_provider_interrupt_issued(
        &self,
        dispatched: &ProviderTurnInterruptEffect,
    ) -> Result<ProviderTurnInterruptEffect, PersistenceError> {
        transition_to_waiting_effect(self, dispatched, None).await
    }

    /// Quarantines a provider interrupt whose dispatch result is uncertain.
    ///
    /// # Errors
    ///
    /// Rejects changed dispatch, execution, or runtime custody authority.
    pub async fn mark_provider_interrupt_ambiguous(
        &self,
        dispatched: &ProviderTurnInterruptEffect,
    ) -> Result<(), PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        let effect_changed = sqlx::query(
            "UPDATE provider_turn_effects SET phase = 'interrupt_ambiguous', updated_at = ? \
             WHERE room_id = ? AND effect_id = ? AND phase = 'dispatching' \
             AND dispatch_nonce = ?",
        )
        .bind(canonical_now())
        .bind(&dispatched.room_id)
        .bind(&dispatched.effect_id)
        .bind(&dispatched.dispatch_nonce)
        .execute(&mut *transaction)
        .await?;
        let execution_changed = sqlx::query(
            "UPDATE provider_turn_executions SET phase = 'interrupt_ambiguous', updated_at = ? \
             WHERE room_id = ? AND session_id = ? AND turn_generation = ? \
             AND execution_id = ? AND phase = 'interrupt_pending' \
             AND runtime_handle_id = ? AND runtime_owner_id = ? AND runtime_lease_token = ?",
        )
        .bind(canonical_now())
        .bind(&dispatched.room_id)
        .bind(&dispatched.session_id)
        .bind(generation_i64(dispatched.turn_generation)?)
        .bind(&dispatched.execution_id)
        .bind(&dispatched.runtime_handle_id)
        .bind(&dispatched.runtime_owner_id)
        .bind(&dispatched.runtime_lease_token)
        .execute(&mut *transaction)
        .await?;
        if effect_changed.rows_affected() != 1 || execution_changed.rows_affected() != 1 {
            return Err(stale_effect());
        }
        transaction.commit().await?;
        Ok(())
    }

    /// Quarantines an interrupt whose exact control or quiescence proof was lost.
    ///
    /// # Errors
    ///
    /// Rejects a stale effect or execution transition.
    pub async fn mark_provider_interrupt_recovery_required(
        &self,
        expected: &ProviderTurnInterruptEffect,
    ) -> Result<(), PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        let current = load_effect_in(
            &mut transaction,
            &expected.room_id,
            &expected.session_id,
            expected.turn_generation,
        )
        .await?;
        require_exact_effect(expected, &current)?;
        let effect_changed = sqlx::query(
            "UPDATE provider_turn_effects SET phase = 'recovery_required', updated_at = ? \
             WHERE room_id = ? AND effect_id = ? \
             AND phase IN ('claimed', 'issued_waiting_quiescence')",
        )
        .bind(canonical_now())
        .bind(&current.room_id)
        .bind(&current.effect_id)
        .execute(&mut *transaction)
        .await?;
        let execution_changed = sqlx::query(
            "UPDATE provider_turn_executions SET phase = 'recovery_required', updated_at = ? \
             WHERE room_id = ? AND session_id = ? AND turn_generation = ? \
             AND execution_id = ? AND phase IN ('interrupt_pending', 'quiescing') \
             AND runtime_handle_id = ? AND runtime_owner_id = ? AND runtime_lease_token = ?",
        )
        .bind(canonical_now())
        .bind(&current.room_id)
        .bind(&current.session_id)
        .bind(generation_i64(current.turn_generation)?)
        .bind(&current.execution_id)
        .bind(&current.runtime_handle_id)
        .bind(&current.runtime_owner_id)
        .bind(&current.runtime_lease_token)
        .execute(&mut *transaction)
        .await?;
        if effect_changed.rows_affected() != 1 || execution_changed.rows_affected() != 1 {
            return Err(stale_effect());
        }
        transaction.commit().await?;
        Ok(())
    }

    /// Loads one durable interrupt effect with its exact execution custody.
    ///
    /// # Errors
    ///
    /// Rejects missing or malformed stored effect authority.
    pub async fn provider_turn_interrupt_effect(
        &self,
        room_id: &str,
        session_id: &str,
        turn_generation: u64,
    ) -> Result<ProviderTurnInterruptEffect, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        let effect = load_effect_in(&mut transaction, room_id, session_id, turn_generation).await?;
        transaction.commit().await?;
        Ok(effect)
    }

    /// Rebinds an unresolved interrupt to an exact live in-memory turn control.
    ///
    /// This transition performs no provider I/O. It is the durable fence that must
    /// commit before the already-installed exact turn token may be signalled again.
    ///
    /// # Errors
    ///
    /// Rejects a stale, terminal, malformed, or no-longer-blocking effect/execution pair.
    pub async fn authorize_provider_interrupt_recovery_wait(
        &self,
        claim: &ProviderTurnEffectClaim,
    ) -> Result<ProviderTurnInterruptEffect, PersistenceError> {
        let expected = &claim.effect;
        let mut transaction = self.pool.begin().await?;
        let mut current = load_effect_in(
            &mut transaction,
            &expected.room_id,
            &expected.session_id,
            expected.turn_generation,
        )
        .await?;
        require_exact_effect(expected, &current)?;
        let execution = load_execution_in(
            &mut transaction,
            &expected.room_id,
            &expected.session_id,
            expected.turn_generation,
        )
        .await?;
        if !recoverable_interrupt_pair(current.phase, execution.phase)
            || execution.execution_id != current.execution_id
            || execution.runtime_handle_id != current.runtime_handle_id
            || execution.runtime_owner_id != current.runtime_owner_id
            || execution.runtime_lease_token != current.runtime_lease_token
            || execution.requeue_finalized
        {
            return Err(stale_effect());
        }
        let now = canonical_now();
        let now_millis = Utc::now().timestamp_millis();
        let effect_changed = sqlx::query(
            "UPDATE provider_turn_effects SET phase = 'issued_waiting_quiescence', updated_at = ? \
             WHERE room_id = ? AND effect_id = ? AND phase = ? AND dispatch_nonce = ? \
             AND claim_owner = ? AND claim_expires_at >= ?",
        )
        .bind(&now)
        .bind(&current.room_id)
        .bind(&current.effect_id)
        .bind(current.phase.as_str())
        .bind(&current.dispatch_nonce)
        .bind(&claim.claim_owner)
        .bind(now_millis)
        .execute(&mut *transaction)
        .await?;
        let execution_changed = sqlx::query(
            "UPDATE provider_turn_executions SET phase = 'quiescing', updated_at = ? \
             WHERE room_id = ? AND session_id = ? AND turn_generation = ? \
             AND execution_id = ? AND phase = ? AND runtime_handle_id = ? \
             AND runtime_owner_id = ? AND runtime_lease_token = ? AND requeue_finalized = 0",
        )
        .bind(&now)
        .bind(&execution.room_id)
        .bind(&execution.session_id)
        .bind(generation_i64(execution.turn_generation)?)
        .bind(&execution.execution_id)
        .bind(execution.phase.as_str())
        .bind(&execution.runtime_handle_id)
        .bind(&execution.runtime_owner_id)
        .bind(&execution.runtime_lease_token)
        .execute(&mut *transaction)
        .await?;
        if effect_changed.rows_affected() != 1 || execution_changed.rows_affected() != 1 {
            return Err(stale_effect());
        }
        transaction.commit().await?;
        current.phase = ProviderTurnEffectPhase::IssuedWaitingQuiescence;
        Ok(current)
    }
}

const fn recoverable_interrupt_pair(
    effect: ProviderTurnEffectPhase,
    execution: ProviderTurnExecutionPhase,
) -> bool {
    match effect {
        ProviderTurnEffectPhase::Prepared => matches!(
            execution,
            ProviderTurnExecutionPhase::InterruptPending
                | ProviderTurnExecutionPhase::StartAmbiguous
                | ProviderTurnExecutionPhase::RecoveryRequired
        ),
        ProviderTurnEffectPhase::Claimed | ProviderTurnEffectPhase::Dispatching => {
            matches!(execution, ProviderTurnExecutionPhase::InterruptPending)
        }
        ProviderTurnEffectPhase::IssuedWaitingQuiescence => {
            matches!(execution, ProviderTurnExecutionPhase::Quiescing)
        }
        ProviderTurnEffectPhase::InterruptAmbiguous => {
            matches!(execution, ProviderTurnExecutionPhase::InterruptAmbiguous)
        }
        ProviderTurnEffectPhase::RecoveryRequired => {
            matches!(execution, ProviderTurnExecutionPhase::RecoveryRequired)
        }
        ProviderTurnEffectPhase::Finalized => false,
    }
}

async fn transition_to_waiting(
    store: &SqliteStore,
    claim: &ProviderTurnEffectClaim,
    dispatched: bool,
) -> Result<ProviderTurnInterruptEffect, PersistenceError> {
    let claim_owner = (!dispatched).then_some(claim.claim_owner.as_str());
    transition_to_waiting_effect(store, &claim.effect, claim_owner).await
}

async fn transition_to_waiting_effect(
    store: &SqliteStore,
    expected: &ProviderTurnInterruptEffect,
    claim_owner: Option<&str>,
) -> Result<ProviderTurnInterruptEffect, PersistenceError> {
    let mut transaction = store.pool.begin().await?;
    let effect_changed = if let Some(claim_owner) = claim_owner {
        sqlx::query(
            "UPDATE provider_turn_effects SET phase = 'issued_waiting_quiescence', \
             updated_at = ? WHERE room_id = ? AND effect_id = ? AND phase = 'claimed' \
             AND claim_owner = ? AND claim_expires_at >= ?",
        )
        .bind(canonical_now())
        .bind(&expected.room_id)
        .bind(&expected.effect_id)
        .bind(claim_owner)
        .bind(Utc::now().timestamp_millis())
        .execute(&mut *transaction)
        .await?
    } else {
        sqlx::query(
            "UPDATE provider_turn_effects SET phase = 'issued_waiting_quiescence', \
             updated_at = ? WHERE room_id = ? AND effect_id = ? AND phase = 'dispatching' \
             AND dispatch_nonce = ?",
        )
        .bind(canonical_now())
        .bind(&expected.room_id)
        .bind(&expected.effect_id)
        .bind(&expected.dispatch_nonce)
        .execute(&mut *transaction)
        .await?
    };
    let execution_changed = sqlx::query(
        "UPDATE provider_turn_executions SET phase = 'quiescing', updated_at = ? \
         WHERE room_id = ? AND session_id = ? AND turn_generation = ? \
         AND execution_id = ? AND phase = 'interrupt_pending' \
         AND runtime_handle_id = ? AND runtime_owner_id = ? AND runtime_lease_token = ?",
    )
    .bind(canonical_now())
    .bind(&expected.room_id)
    .bind(&expected.session_id)
    .bind(generation_i64(expected.turn_generation)?)
    .bind(&expected.execution_id)
    .bind(&expected.runtime_handle_id)
    .bind(&expected.runtime_owner_id)
    .bind(&expected.runtime_lease_token)
    .execute(&mut *transaction)
    .await?;
    if effect_changed.rows_affected() != 1 || execution_changed.rows_affected() != 1 {
        return Err(stale_effect());
    }
    transaction.commit().await?;
    store
        .provider_turn_interrupt_effect(
            &expected.room_id,
            &expected.session_id,
            expected.turn_generation,
        )
        .await
}

pub(crate) async fn load_optional_effect_in(
    transaction: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    session_id: &str,
    turn_generation: u64,
) -> Result<Option<ProviderTurnInterruptEffect>, PersistenceError> {
    let exists = sqlx::query_scalar::<_, i64>(
        "SELECT EXISTS(SELECT 1 FROM provider_turn_effects WHERE room_id = ? \
         AND session_id = ? AND turn_generation = ? AND effect_kind = 'interrupt')",
    )
    .bind(room_id)
    .bind(session_id)
    .bind(generation_i64(turn_generation)?)
    .fetch_one(&mut **transaction)
    .await?
        != 0;
    if !exists {
        return Ok(None);
    }
    load_effect_in(transaction, room_id, session_id, turn_generation)
        .await
        .map(Some)
}

pub(crate) async fn load_effect_in(
    transaction: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    session_id: &str,
    turn_generation: u64,
) -> Result<ProviderTurnInterruptEffect, PersistenceError> {
    let row = sqlx::query(
        "SELECT effect.effect_id, effect.phase, effect.dispatch_nonce, \
         execution.execution_id, execution.participant_id, execution.turn_id, \
         execution.start_dispatch_nonce, execution.runtime_handle_id, \
         execution.runtime_owner_id, execution.runtime_lease_token \
         FROM provider_turn_effects effect JOIN provider_turn_executions execution \
         ON execution.room_id = effect.room_id AND execution.session_id = effect.session_id \
         AND execution.turn_generation = effect.turn_generation \
         WHERE effect.room_id = ? AND effect.session_id = ? \
         AND effect.turn_generation = ? AND effect.effect_kind = 'interrupt'",
    )
    .bind(room_id)
    .bind(session_id)
    .bind(generation_i64(turn_generation)?)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or_else(stale_effect)?;
    Ok(ProviderTurnInterruptEffect {
        room_id: room_id.to_owned(),
        session_id: session_id.to_owned(),
        turn_generation,
        effect_id: row.get("effect_id"),
        phase: ProviderTurnEffectPhase::parse(row.get::<String, _>("phase").as_str())?,
        execution_id: row.get("execution_id"),
        participant_id: row.get("participant_id"),
        turn_id: row.get("turn_id"),
        start_dispatch_nonce: row.get("start_dispatch_nonce"),
        runtime_handle_id: row.get("runtime_handle_id"),
        runtime_owner_id: row.get("runtime_owner_id"),
        runtime_lease_token: row.get("runtime_lease_token"),
        dispatch_nonce: row.get("dispatch_nonce"),
    })
}

pub(crate) fn require_exact_effect(
    expected: &ProviderTurnInterruptEffect,
    current: &ProviderTurnInterruptEffect,
) -> Result<(), PersistenceError> {
    if expected.room_id != current.room_id
        || expected.session_id != current.session_id
        || expected.turn_generation != current.turn_generation
        || expected.effect_id != current.effect_id
        || expected.execution_id != current.execution_id
        || expected.participant_id != current.participant_id
        || expected.turn_id != current.turn_id
        || expected.start_dispatch_nonce != current.start_dispatch_nonce
        || expected.runtime_handle_id != current.runtime_handle_id
        || expected.runtime_owner_id != current.runtime_owner_id
        || expected.runtime_lease_token != current.runtime_lease_token
        || expected.dispatch_nonce != current.dispatch_nonce
    {
        return Err(stale_effect());
    }
    Ok(())
}

fn validate_claim_owner(value: &str) -> Result<(), PersistenceError> {
    Uuid::parse_str(value).map_err(|_| invalid_effect())?;
    Ok(())
}

pub(crate) fn generation_i64(generation: u64) -> Result<i64, PersistenceError> {
    i64::try_from(generation).map_err(|_| invalid_effect())
}

pub(crate) fn canonical_now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Micros, true)
}

pub(crate) fn invalid_effect() -> PersistenceError {
    PersistenceError::CommandRejected {
        code: "stored_turn_effect_invalid",
        message: "Stored provider turn effect authority is invalid.".to_owned(),
    }
}

pub(crate) fn stale_effect() -> PersistenceError {
    PersistenceError::CommandRejected {
        code: "stale_provider_turn_effect",
        message: "Provider turn effect authority changed before this operation.".to_owned(),
    }
}

fn unresolved_effect() -> PersistenceError {
    PersistenceError::CommandUnresolved {
        code: "provider_turn_effect_unresolved",
        message: "The exact provider turn effect is owned by recovery or remains unresolved."
            .to_owned(),
    }
}
