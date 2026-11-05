//! v103 rebinds each stored workspace and executable identity once to the
//! reboot-stable form, which drops the Unix device number macOS renumbers at
//! boot. Only an identity whose stored path still canonicalizes to itself is
//! rebound, and only in a session whose profile key still matches its own
//! fields; the key is then recomputed over the rebound identities. Any other
//! session keeps its old values and fails closed as before.
use agentsassemble_domain::{DurableAgentSession, durable_runtime_profile_key};
use sqlx::SqlitePool;

use crate::{PersistenceError, filesystem_authority::reboot_stable_identities};

pub(crate) async fn upgrade(pool: &SqlitePool) -> Result<(), PersistenceError> {
    // Filesystem work runs before the write transaction, so it never holds the writer.
    let rows: Vec<(String, String, String)> =
        sqlx::query_as("SELECT room_id, session_id, session_json FROM agent_sessions")
            .fetch_all(pool)
            .await?;
    let mut rebound = Vec::new();
    for (room_id, session_id, encoded) in rows {
        let mut session: DurableAgentSession = serde_json::from_str(&encoded)?;
        if durable_runtime_profile_key(&session) != session.runtime_profile_key {
            continue;
        }
        let (workspace, executable) = reboot_stable_identities(
            session.public.provider_kind.clone(),
            session.workspace.clone(),
            session.executable.clone(),
        )
        .await;
        let mut changed = false;
        for (stored, current) in [
            (&mut session.workspace_identity, workspace),
            (&mut session.executable_identity, executable),
        ] {
            if let Some(current) = current
                && *stored != current
            {
                *stored = current;
                changed = true;
            }
        }
        if changed {
            session.runtime_profile_key = durable_runtime_profile_key(&session);
            rebound.push((
                room_id,
                session_id,
                encoded,
                serde_json::to_string(&session)?,
            ));
        }
    }
    let mut tx = pool.begin().await?;
    let version: String =
        sqlx::query_scalar("SELECT value FROM runtime_metadata WHERE key = 'schema_version'")
            .fetch_one(&mut *tx)
            .await?;
    if version.parse::<i64>().is_ok_and(|version| version < 103) {
        for (room_id, session_id, read, written) in rebound {
            // A row that changed since it was read keeps its own identities.
            sqlx::query(
                "UPDATE agent_sessions SET session_json = ? \
                 WHERE room_id = ? AND session_id = ? AND session_json = ?",
            )
            .bind(written)
            .bind(room_id)
            .bind(session_id)
            .bind(read)
            .execute(&mut *tx)
            .await?;
        }
        sqlx::query("UPDATE runtime_metadata SET value = '103' WHERE key = 'schema_version'")
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}

#[cfg(test)]
#[path = "file_identity_rebind_tests.rs"]
mod tests;
