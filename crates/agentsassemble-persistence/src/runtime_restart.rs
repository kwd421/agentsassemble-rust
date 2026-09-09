use agentsassemble_domain::{AgentRuntimeStatus, AgentSessionStatus, DurableAgentSession};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{Row, Sqlite, Transaction};

use crate::{PersistenceError, SqliteStore};

const RESTART_KEY: &str = "runtime_restart_v1";

pub use agentsassemble_domain::RuntimeRestartPhase;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeRestartTarget {
    pub room_id: String,
    pub session_id: String,
    pub paused: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeRestartRecord {
    pub operation_id: String,
    pub phase: RuntimeRestartPhase,
    pub updated_at: DateTime<Utc>,
    pub targets: Vec<RuntimeRestartTarget>,
    pub(crate) source_generation: String,
    pub(crate) recovery_generation: Option<String>,
    pub(crate) candidate_identity: Option<String>,
}

impl SqliteStore {
    /// Reads the latest durable restart receipt, without implying current readiness.
    ///
    /// # Errors
    /// Returns storage or malformed-state failures.
    pub async fn runtime_restart_status(
        &self,
    ) -> Result<Option<RuntimeRestartRecord>, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        let record = load_restart(&mut transaction).await?;
        transaction.commit().await?;
        Ok(record)
    }

    /// Reads the requested receipt, including immutable retired operations.
    ///
    /// # Errors
    /// Returns storage or malformed-state failures.
    pub async fn runtime_restart_operation(
        &self,
        operation_id: &str,
    ) -> Result<Option<RuntimeRestartRecord>, PersistenceError> {
        let mut tx = self.pool.begin().await?;
        let current = load_restart(&mut tx).await?;
        let record = if current
            .as_ref()
            .is_some_and(|record| record.operation_id == operation_id)
        {
            current
        } else {
            load_at(&mut tx, &format!("{RESTART_KEY}:{operation_id}")).await?
        };
        tx.commit().await?;
        Ok(record)
    }

    /// Atomically refuses busy work or prevents new work under one restart receipt.
    /// The caller owns candidate preparation and the subsequent transition.
    ///
    /// # Errors
    /// Returns invalid operation, busy runtime, corrupt authority or storage failures.
    pub async fn prepare_runtime_restart(
        &self,
        operation_id: &str,
    ) -> Result<RuntimeRestartRecord, PersistenceError> {
        uuid::Uuid::parse_str(operation_id).map_err(|_| {
            rejected(
                "runtime_restart_invalid",
                "The restart operation ID is invalid.",
            )
        })?;
        let mut transaction = self.pool.begin().await?;
        if let Some(retired) =
            load_at(&mut transaction, &format!("{RESTART_KEY}:{operation_id}")).await?
        {
            transaction.commit().await?;
            return Ok(retired);
        }
        if let Some(previous) = load_restart(&mut transaction).await? {
            if previous.operation_id == operation_id {
                transaction.commit().await?;
                return Ok(previous);
            }
            if previous.phase.blocks_admission() {
                return Err(busy());
            }
            // Retired receipts are immutable; only RESTART_KEY owns the active transition.
            save_at(
                &mut transaction,
                &format!("{RESTART_KEY}:{}", previous.operation_id),
                &previous,
            )
            .await?;
        }
        let targets = quiescent_targets(&mut transaction).await?;
        let record = RuntimeRestartRecord {
            operation_id: operation_id.to_owned(),
            phase: RuntimeRestartPhase::Quiescing,
            updated_at: Utc::now(),
            targets,
            source_generation: self.runtime_generation().to_owned(),
            recovery_generation: None,
            candidate_identity: None,
        };
        save_restart(&mut transaction, &record).await?;
        transaction.commit().await?;
        Ok(record)
    }

    /// Releases a preparation owned by this exact runtime. Retrying is idempotent.
    ///
    /// # Errors
    /// Returns mismatched ownership, malformed-state or storage failures.
    pub async fn abort_runtime_restart(
        &self,
        operation_id: &str,
    ) -> Result<RuntimeRestartRecord, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        let mut record = load_restart(&mut transaction).await?.ok_or_else(stale)?;
        if record.operation_id != operation_id
            || record.source_generation != self.runtime_generation()
        {
            return Err(stale());
        }
        if !matches!(
            record.phase,
            RuntimeRestartPhase::Quiescing | RuntimeRestartPhase::Aborted
        ) {
            return Err(stale());
        }
        if record.phase == RuntimeRestartPhase::Quiescing {
            record.phase = RuntimeRestartPhase::Aborted;
            record.updated_at = Utc::now();
            save_restart(&mut transaction, &record).await?;
        }
        transaction.commit().await?;
        Ok(record)
    }
}

pub(crate) async fn restart_quiescing(
    transaction: &mut Transaction<'_, Sqlite>,
) -> Result<bool, PersistenceError> {
    Ok(load_restart(transaction)
        .await?
        .is_some_and(|record| record.phase.blocks_admission()))
}

pub(crate) async fn require_runtime_admission(
    transaction: &mut Transaction<'_, Sqlite>,
) -> Result<(), PersistenceError> {
    if restart_quiescing(transaction).await? {
        return Err(PersistenceError::CommandUnresolved {
            code: "runtime_restarting".into(),
            message:
                "The runtime is quiescing for restart. Retry the same request after readiness."
                    .to_owned(),
        });
    }
    Ok(())
}

pub(crate) async fn load_restart(
    transaction: &mut Transaction<'_, Sqlite>,
) -> Result<Option<RuntimeRestartRecord>, PersistenceError> {
    load_at(transaction, RESTART_KEY).await
}

async fn load_at(
    transaction: &mut Transaction<'_, Sqlite>,
    key: &str,
) -> Result<Option<RuntimeRestartRecord>, PersistenceError> {
    let stored: Option<String> =
        sqlx::query_scalar("SELECT value FROM runtime_metadata WHERE key = ?")
            .bind(key)
            .fetch_optional(&mut **transaction)
            .await?;
    stored
        .map(|value| serde_json::from_str(&value).map_err(PersistenceError::from))
        .transpose()
}

pub(crate) async fn save_restart(
    transaction: &mut Transaction<'_, Sqlite>,
    record: &RuntimeRestartRecord,
) -> Result<(), PersistenceError> {
    save_at(transaction, RESTART_KEY, record).await
}

async fn save_at(
    transaction: &mut Transaction<'_, Sqlite>,
    key: &str,
    record: &RuntimeRestartRecord,
) -> Result<(), PersistenceError> {
    sqlx::query("INSERT INTO runtime_metadata (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value")
        .bind(key).bind(serde_json::to_string(record)?).execute(&mut **transaction).await?;
    Ok(())
}

async fn quiescent_targets(
    transaction: &mut Transaction<'_, Sqlite>,
) -> Result<Vec<RuntimeRestartTarget>, PersistenceError> {
    let pending: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM lifecycle_command_reservations WHERE status = 'pending')",
    )
    .fetch_one(&mut **transaction)
    .await?;
    if pending {
        return Err(busy());
    }
    let rows = sqlx::query("SELECT session_json FROM agent_sessions ORDER BY room_id, session_id")
        .fetch_all(&mut **transaction)
        .await?;
    let mut targets = Vec::new();
    for row in rows {
        let session: DurableAgentSession = serde_json::from_str(row.get("session_json"))?;
        if !session.lifecycle_intent_action.is_none()
            || !session.lifecycle_intent_status.is_none()
            || session.public.recovery_required
            || crate::turn_authority::active_turn_authority(&session).map_err(|_| {
                rejected(
                    "stored_turn_authority_invalid",
                    "Stored Agent Session turn authority is inconsistent.",
                )
            })?
            || matches!(
                session.public.runtime_status,
                AgentRuntimeStatus::Starting
                    | AgentRuntimeStatus::Busy
                    | AgentRuntimeStatus::Stopping
                    | AgentRuntimeStatus::Recovering
            )
            || crate::provider_turn_execution::blocking_execution_exists(
                transaction,
                &session.public.room_id,
                &session.public.session_id,
            )
            .await?
        {
            return Err(busy());
        }
        if session.public.status == AgentSessionStatus::Attached
            && (session.public.enabled
                || session.public.runtime_status == AgentRuntimeStatus::Paused)
            && session.public.provider_session_active
            && !session.public.external_owned
            && session.public.process_ownership == "server"
            && matches!(
                session.public.runtime_status,
                AgentRuntimeStatus::Idle | AgentRuntimeStatus::Paused
            )
        {
            targets.push(RuntimeRestartTarget {
                room_id: session.public.room_id,
                session_id: session.public.session_id,
                paused: session.public.runtime_status == AgentRuntimeStatus::Paused,
            });
        }
    }
    Ok(targets)
}

fn rejected(code: &'static str, message: &str) -> PersistenceError {
    PersistenceError::CommandRejected {
        code: code.into(),
        message: message.to_owned(),
    }
}

fn busy() -> PersistenceError {
    rejected(
        "runtime_restart_busy",
        "The runtime has active or unresolved work and cannot restart.",
    )
}

pub(crate) fn stale() -> PersistenceError {
    rejected(
        "runtime_restart_stale",
        "This runtime does not own the requested restart transition.",
    )
}
