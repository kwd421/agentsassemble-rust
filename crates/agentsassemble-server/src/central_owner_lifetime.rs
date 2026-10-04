//! One cancellable renewal per connected owner, retained by its streams/sockets.
use std::{
    collections::HashMap,
    sync::{
        Arc, Weak,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use agentsassemble_persistence::{
    OwnerSessionAuthorization, PersistenceError, RoomSessionAuthorization,
};
use agentsassemble_protocol::{CentralOwnerSessionEnd, CentralOwnerSessionStatus};
use parking_lot::Mutex;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

use crate::{AppState, central_directory::CentralDirectoryError};

#[derive(Clone, Default)]
pub(crate) struct OwnerSessionLifetimes(Arc<Mutex<HashMap<[u8; 32], Weak<LiveOwner>>>>);

struct LiveOwner {
    holders: AtomicUsize,
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
        let _entries = self.lifetimes.0.lock();
        if self.owner.holders.fetch_sub(1, Ordering::AcqRel) == 1 {
            self.owner.cancel.cancel();
        }
    }
}

impl OwnerSessionLifetimes {
    pub(crate) fn require_live(
        &self,
        owner: &OwnerSessionAuthorization,
    ) -> Result<(), PersistenceError> {
        let entries = self.0.lock();
        if entries
            .get(owner.fingerprint())
            .and_then(Weak::upgrade)
            .is_some_and(|entry| {
                matches!(
                    *entry.status.borrow(),
                    CentralOwnerSessionStatus::Ended { .. }
                )
            })
        {
            return Err(invalid());
        }
        Ok(())
    }

    pub(crate) fn retain(
        &self,
        state: &AppState,
        authorization: OwnerSessionAuthorization,
    ) -> Result<OwnerSessionLease, PersistenceError> {
        let mut entries = self.0.lock();
        entries.retain(|_, entry| entry.strong_count() != 0);
        if entries
            .get(authorization.fingerprint())
            .and_then(Weak::upgrade)
            .is_some_and(|entry| {
                matches!(
                    *entry.status.borrow(),
                    CentralOwnerSessionStatus::Ended { .. }
                )
            })
        {
            return Err(invalid());
        }
        let entry = if let Some(entry) = entries
            .get(authorization.fingerprint())
            .and_then(Weak::upgrade)
            .filter(|entry| !entry.cancel.is_cancelled())
        {
            entry.holders.fetch_add(1, Ordering::AcqRel);
            entry
        } else {
            let (status, _) = watch::channel(CentralOwnerSessionStatus::Active {
                expires_at: authorization.expires_at(),
            });
            let entry = Arc::new(LiveOwner {
                holders: AtomicUsize::new(1),
                cancel: CancellationToken::new(),
                status,
            });
            entries.insert(*authorization.fingerprint(), Arc::downgrade(&entry));
            state
                .connections
                .spawn(renew(state.clone(), authorization, entry.clone()));
            entry
        };
        Ok(OwnerSessionLease {
            lifetimes: self.clone(),
            status: entry.status.subscribe(),
            owner: entry,
        })
    }
}

pub(crate) async fn retain_room_owner(
    state: &AppState,
    room: Option<&RoomSessionAuthorization>,
) -> Result<Option<OwnerSessionLease>, PersistenceError> {
    let Some(RoomSessionAuthorization::Operator(session)) = room else {
        return Ok(None);
    };
    state
        .store
        .owner_for_operator_session(session)
        .await?
        .map(|owner| state.owner_sessions.retain(state, owner))
        .transpose()
}

fn invalid() -> PersistenceError {
    PersistenceError::CommandRejected {
        code: "central_owner_session_invalid".into(),
        message: "Server owner access is no longer available.".into(),
    }
}

async fn end(
    state: &AppState,
    authorization: &OwnerSessionAuthorization,
    entry: &LiveOwner,
    reason: CentralOwnerSessionEnd,
) {
    if let Err(error) = state
        .store
        .revoke_owner_session(authorization.fingerprint())
        .await
    {
        tracing::error!(error = ?error, "owner session revocation could not be persisted");
        state.shutdown.cancel();
        entry.status.send_replace(CentralOwnerSessionStatus::Ended {
            reason: CentralOwnerSessionEnd::Unavailable,
        });
    } else {
        entry
            .status
            .send_replace(CentralOwnerSessionStatus::Ended { reason });
    }
}

async fn renew(
    state: AppState,
    mut authorization: OwnerSessionAuthorization,
    entry: Arc<LiveOwner>,
) {
    let binding = authorization.binding();
    let Ok(current) = state
        .store
        .authorize_owner_session(
            authorization.fingerprint(),
            &binding.browser_fingerprint,
            &binding.origin,
        )
        .await
    else {
        end(
            &state,
            &authorization,
            &entry,
            CentralOwnerSessionEnd::Unavailable,
        )
        .await;
        return;
    };
    authorization = current;
    renew_live(state, authorization, entry).await;
}

async fn renew_live(
    state: AppState,
    mut authorization: OwnerSessionAuthorization,
    entry: Arc<LiveOwner>,
) {
    let mut retry = 0_u32;
    let mut due = authorization.renew_at();
    loop {
        let expires = authorization.expires_at();
        let now = chrono::Utc::now().timestamp();
        let expires_in = Duration::from_secs(expires.saturating_sub(now).max(0).cast_unsigned());
        let until_due = Duration::from_secs(due.saturating_sub(now).max(0).cast_unsigned());
        tokio::select! {
            () = state.shutdown.cancelled() => return,
            () = entry.cancel.cancelled() => return,
            () = tokio::time::sleep(expires_in) => { end(&state, &authorization, &entry, CentralOwnerSessionEnd::Expired).await; return; },
            () = tokio::time::sleep(until_due) => {}
        }
        let binding = authorization.binding();
        if state
            .public_ingress
            .ready_snapshot()
            .is_none_or(|ready| ready.public_url != binding.origin)
        {
            end(
                &state,
                &authorization,
                &entry,
                CentralOwnerSessionEnd::Revoked,
            )
            .await;
            return;
        }
        let remaining = Duration::from_secs(
            expires
                .saturating_sub(chrono::Utc::now().timestamp())
                .max(0)
                .cast_unsigned(),
        );
        let result = tokio::select! {
            () = state.shutdown.cancelled() => return,
            () = entry.cancel.cancelled() => return,
            () = tokio::time::sleep(remaining) => { end(&state, &authorization, &entry, CentralOwnerSessionEnd::Expired).await; return; },
            result = state.central_directory.owner_connection(&state.central_host_identity, "connection_id",
                &binding.connection_id, &binding.origin, binding.generation, &binding.browser_fingerprint) => result
        };
        match result {
            Ok(lease) => {
                if let Ok(current) = state
                    .store
                    .renew_owner_session(&authorization, &lease)
                    .await
                {
                    authorization = current;
                    due = authorization.renew_at();
                    retry = 0;
                    entry
                        .status
                        .send_replace(CentralOwnerSessionStatus::Active {
                            expires_at: authorization.expires_at(),
                        });
                } else {
                    end(
                        &state,
                        &authorization,
                        &entry,
                        CentralOwnerSessionEnd::Unavailable,
                    )
                    .await;
                    return;
                }
            }
            Err(CentralDirectoryError::Request(_) | CentralDirectoryError::Unavailable) => {
                entry
                    .status
                    .send_replace(CentralOwnerSessionStatus::Retrying {
                        expires_at: expires,
                    });
                retry = retry.saturating_add(1);
                due = chrono::Utc::now().timestamp() + 2_i64.pow(retry.min(3));
            }
            Err(_) => {
                end(
                    &state,
                    &authorization,
                    &entry,
                    CentralOwnerSessionEnd::Revoked,
                )
                .await;
                return;
            }
        }
    }
}
