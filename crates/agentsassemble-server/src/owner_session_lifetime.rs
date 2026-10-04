//! Host-owned live admission. No central heartbeat, lease renewal or connected-session timer.
use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use agentsassemble_persistence::{
    OwnerSessionAuthorization, PersistenceError, RoomSessionAuthorization,
};
use agentsassemble_protocol::{CentralOwnerSessionEnd, CentralOwnerSessionStatus};
use parking_lot::Mutex;
use tokio::sync::{Notify, watch};
use tokio_util::sync::CancellationToken;

use crate::AppState;

#[derive(Clone, Default)]
pub(crate) struct OwnerSessionLifetimes(Arc<Mutex<HashMap<[u8; 32], Arc<LiveOwner>>>>);

struct LiveOwner {
    holders: AtomicUsize,
    ever_connected: AtomicBool,
    opening_until: i64,
    first_connection: Notify,
    cancel: CancellationToken,
    status: watch::Sender<CentralOwnerSessionStatus>,
}

pub(crate) struct OwnerSessionLease {
    lifetimes: OwnerSessionLifetimes,
    owner: Arc<LiveOwner>,
    pub(crate) status: watch::Receiver<CentralOwnerSessionStatus>,
}

impl Drop for OwnerSessionLease {
    fn drop(&mut self) {
        // Retaining and last-disconnect cancellation share the same lock.
        let _entries = self.lifetimes.0.lock();
        if self.owner.holders.fetch_sub(1, Ordering::AcqRel) == 1 {
            self.owner.cancel.cancel();
        }
    }
}

impl OwnerSessionLifetimes {
    pub(crate) fn admit(
        &self,
        state: &AppState,
        owner: OwnerSessionAuthorization,
    ) -> Result<(), PersistenceError> {
        let mut entries = self.0.lock();
        if let Some(entry) = entries.get(owner.fingerprint()) {
            return require_active(entry);
        }
        let ingress = state
            .public_ingress
            .ready_lifetime(&owner.binding().origin)
            .ok_or_else(invalid)?;
        let (status, _) = watch::channel(CentralOwnerSessionStatus::Active {});
        let entry = Arc::new(LiveOwner {
            holders: AtomicUsize::new(0),
            ever_connected: AtomicBool::new(false),
            opening_until: owner.admission_expires_at(),
            first_connection: Notify::new(),
            cancel: CancellationToken::new(),
            status,
        });
        entries.insert(*owner.fingerprint(), entry.clone());
        state
            .connections
            .spawn(settle_connection(state.clone(), owner, entry, ingress));
        Ok(())
    }

    pub(crate) fn require_live(
        &self,
        owner: &OwnerSessionAuthorization,
    ) -> Result<(), PersistenceError> {
        let entries = self.0.lock();
        require_active(entries.get(owner.fingerprint()).ok_or_else(invalid)?)
    }

    pub(crate) fn retain(
        &self,
        owner: &OwnerSessionAuthorization,
    ) -> Result<OwnerSessionLease, PersistenceError> {
        let entries = self.0.lock();
        let entry = entries.get(owner.fingerprint()).ok_or_else(invalid)?;
        require_active(entry)?;
        entry.ever_connected.store(true, Ordering::Release);
        entry.holders.fetch_add(1, Ordering::AcqRel);
        entry.first_connection.notify_one();
        Ok(OwnerSessionLease {
            lifetimes: self.clone(),
            owner: entry.clone(),
            status: entry.status.subscribe(),
        })
    }

    /// Called only with the fingerprints returned by committed host revocation.
    pub(crate) fn revoke(&self, fingerprints: &[[u8; 32]]) {
        let entries = self.0.lock();
        for fingerprint in fingerprints {
            if let Some(entry) = entries.get(fingerprint) {
                entry.status.send_replace(CentralOwnerSessionStatus::Ended {
                    reason: CentralOwnerSessionEnd::Revoked,
                });
                entry.cancel.cancel();
            }
        }
    }
}

pub(crate) async fn retain_room_owner(
    state: &AppState,
    session: Option<&RoomSessionAuthorization>,
) -> Result<Option<OwnerSessionLease>, PersistenceError> {
    let Some(RoomSessionAuthorization::Operator(session)) = session else {
        return Ok(None);
    };
    state
        .store
        .owner_for_operator_session(session)
        .await?
        .map(|owner| state.owner_sessions.retain(&owner))
        .transpose()
}

fn require_active(entry: &LiveOwner) -> Result<(), PersistenceError> {
    if entry.cancel.is_cancelled()
        || (!entry.ever_connected.load(Ordering::Acquire)
            && entry.opening_until <= chrono::Utc::now().timestamp())
        || matches!(
            *entry.status.borrow(),
            CentralOwnerSessionStatus::Ended { .. }
        )
    {
        return Err(invalid());
    }
    Ok(())
}

fn invalid() -> PersistenceError {
    PersistenceError::CommandRejected {
        code: "central_owner_session_invalid".into(),
        message: "The host owner connection has ended.".into(),
    }
}

async fn settle_connection(
    state: AppState,
    owner: OwnerSessionAuthorization,
    entry: Arc<LiveOwner>,
    ingress: CancellationToken,
) {
    // The only timer is the already-short entry window before the first transport.
    // Once retained, authority ends on explicit local events, never central time.
    let opening = Duration::from_secs(
        owner
            .admission_expires_at()
            .saturating_sub(chrono::Utc::now().timestamp())
            .max(0)
            .cast_unsigned(),
    );
    let connected = tokio::select! {
        biased;
        () = state.shutdown.cancelled() => false,
        () = entry.cancel.cancelled() => false,
        () = ingress.cancelled() => false,
        () = entry.first_connection.notified() => true,
        () = tokio::time::sleep(opening) => false,
    };
    if connected {
        tokio::select! { () = state.shutdown.cancelled() => {}, () = entry.cancel.cancelled() => {}, () = ingress.cancelled() => {} }
    }
    entry.cancel.cancel();
    let reason = if state
        .store
        .disconnect_owner_session(owner.fingerprint())
        .await
        .is_err()
    {
        tracing::error!("host owner disconnect did not commit");
        state.shutdown.cancel();
        CentralOwnerSessionEnd::Unavailable
    } else {
        CentralOwnerSessionEnd::Disconnected
    };
    if !matches!(
        *entry.status.borrow(),
        CentralOwnerSessionStatus::Ended {
            reason: CentralOwnerSessionEnd::Revoked
        }
    ) {
        entry
            .status
            .send_replace(CentralOwnerSessionStatus::Ended { reason });
    }
    let mut entries = state.owner_sessions.0.lock();
    if entries
        .get(owner.fingerprint())
        .is_some_and(|current| Arc::ptr_eq(current, &entry))
    {
        entries.remove(owner.fingerprint());
    }
}
