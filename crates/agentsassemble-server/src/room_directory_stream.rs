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
        token: String,
        generation: i64,
        origin: String,
        device: [u8; 32],
        expires_at: chrono::DateTime<chrono::Utc>,
    },
}

impl DirectoryStreamAuthority {
    async fn validate(&self, state: &AppState) -> Result<(), io::Error> {
        let owner = match self {
            Self::Local => ServerOwnerAuthority::LocalOperator,
            Self::Central {
                token,
                generation,
                origin,
                device,
                ..
            } => {
                crate::central_owner_web::redeem_directory_owner(
                    state,
                    token,
                    origin,
                    *generation,
                    *device,
                )
                .await
                .map_err(|_| io::Error::other("Directory ownership is unavailable."))?
                .0
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

    async fn expired(&self) {
        match self {
            Self::Local => std::future::pending().await,
            Self::Central { expires_at, .. } => {
                let remaining = (*expires_at - chrono::Utc::now())
                    .to_std()
                    .unwrap_or(Duration::ZERO);
                tokio::time::sleep(remaining).await;
            }
        }
    }
}

// The HTTP body owns both the subscription and admission lease. Dropping the
// fetch, expiring the grant or shutting down releases them without a spawned task.
pub(crate) fn directory_stream(
    state: AppState,
    changes: watch::Receiver<()>,
    authority: DirectoryStreamAuthority,
    lease: ConnectionLease,
    transport_wait: AuthenticatedHttpWait,
) -> Response {
    let stream = stream::unfold(
        (state, changes, authority, lease, transport_wait, true),
        |(state, mut changes, authority, lease, transport_wait, initial)| async move {
            if !initial {
                tokio::select! {
                    () = state.shutdown.cancelled() => return None,
                    () = authority.expired() => return None,
                    result = changes.changed() => if result.is_err() { return None; },
                }
            }
            // Revalidate immediately before every data frame, including admission;
            // comments carry no authority and the deadline also applies while idle.
            if state.shutdown.is_cancelled() {
                return None;
            }
            let validation = tokio::select! {
                () = state.shutdown.cancelled() => return None,
                () = authority.expired() => return None,
                result = authority.validate(&state) => result,
            };
            if let Err(error) = validation {
                return Some((
                    Err(error),
                    (state, changes, authority, lease, transport_wait, false),
                ));
            }
            Some((
                Ok(Event::default().event("directory_changed").data("{}")),
                (state, changes, authority, lease, transport_wait, false),
            ))
        },
    );
    Sse::new(stream)
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
        .into_response()
}
