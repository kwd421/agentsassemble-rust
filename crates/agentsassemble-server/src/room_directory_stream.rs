use std::{io, time::Duration};

use agentsassemble_persistence::{PersistenceError, ServerOwnerAuthority};
use axum::response::{
    IntoResponse, Response, Sse,
    sse::{Event, KeepAlive},
};
use futures_util::stream;
use tokio::sync::watch;

use crate::{
    AppState, connection_admission::ConnectionLease, http_admission::AuthenticatedHttpWait,
};

pub(crate) enum DirectoryStreamAuthority {
    Local,
    Central {
        owner: Box<agentsassemble_persistence::OwnerSessionAuthorization>,
        lease: crate::central_owner_lifetime::OwnerSessionLease,
    },
}

impl DirectoryStreamAuthority {
    async fn validate(&self, state: &AppState) -> Result<(), io::Error> {
        let owner = match self {
            Self::Local => ServerOwnerAuthority::LocalOperator,
            Self::Central { owner, .. } => {
                if state
                    .public_ingress
                    .ready_snapshot()
                    .is_none_or(|ready| ready.public_url != owner.binding().origin)
                {
                    return Err(io::Error::other("Directory ownership is unavailable."));
                }
                state
                    .owner_sessions
                    .require_live(owner)
                    .map_err(|_| io::Error::other("Directory ownership is unavailable."))?;
                ServerOwnerAuthority::CentralSession(*owner.clone())
            }
        };
        state
            .store
            .validate_server_owner(&owner)
            .await
            .map_err(|_: PersistenceError| {
                io::Error::other("Directory ownership is unavailable.")
            })?;
        Ok(())
    }

    async fn changed(&mut self) -> Result<(), watch::error::RecvError> {
        match self {
            Self::Local => std::future::pending().await,
            Self::Central { lease, .. } => lease.status.changed().await,
        }
    }
}

// The authenticated HTTP body retains its owner renewal. Status frames carry no
// directory data; a successful renewal leaves this exact connection open.
pub(crate) fn directory_stream(
    state: AppState,
    changes: watch::Receiver<()>,
    authority: DirectoryStreamAuthority,
    lease: ConnectionLease,
    transport_wait: AuthenticatedHttpWait,
) -> Response {
    let stream = stream::unfold(
        (
            state,
            changes,
            authority,
            lease,
            transport_wait,
            0_u8,
            false,
        ),
        |(state, mut changes, mut authority, lease, transport_wait, mut phase, closed)| async move {
            if closed || state.shutdown.is_cancelled() {
                return None;
            }
            let status_frame = if phase == 0
                && matches!(&authority, DirectoryStreamAuthority::Central { .. })
            {
                phase = 1;
                true
            } else if phase < 2 {
                phase = 2;
                false
            } else {
                tokio::select! {
                    () = state.shutdown.cancelled() => return None,
                    result = changes.changed() => { if result.is_err() { return None; } false },
                    result = authority.changed() => { if result.is_err() { return None; } true },
                }
            };
            let mut terminal = false;
            let frame = if status_frame {
                let DirectoryStreamAuthority::Central { lease: owner, .. } = &mut authority else {
                    return None;
                };
                let status = owner.status.borrow_and_update().clone();
                terminal = matches!(
                    status,
                    agentsassemble_protocol::CentralOwnerSessionStatus::Ended { .. }
                );
                Event::default()
                    .event("owner_session")
                    .json_data(status)
                    .map_err(io::Error::other)
            } else if let Err(error) = authority.validate(&state).await {
                if matches!(&authority, DirectoryStreamAuthority::Central { .. }) {
                    terminal = true;
                    Event::default()
                        .event("owner_session")
                        .json_data(agentsassemble_protocol::CentralOwnerSessionStatus::Ended {
                            reason: agentsassemble_protocol::CentralOwnerSessionEnd::Unavailable,
                        })
                        .map_err(io::Error::other)
                } else {
                    Err(error)
                }
            } else {
                Ok(Event::default().event("directory_changed").data("{}"))
            };
            Some((
                frame,
                (
                    state,
                    changes,
                    authority,
                    lease,
                    transport_wait,
                    phase,
                    terminal,
                ),
            ))
        },
    );
    Sse::new(stream)
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
        .into_response()
}
