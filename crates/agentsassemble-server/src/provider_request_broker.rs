//! Room-actor custody of live responses. Persistence remains the authority for every transition.
use std::{
    collections::{HashMap, HashSet},
    future::Future,
    pin::Pin,
};

use agentsassemble_domain::{AuthenticatedPrincipal, ProviderRequestResolution, RoomEvent};
use agentsassemble_persistence::{
    AttendeeConnectionAuthorization, OpenProviderRequest, PersistenceError, ProviderRequestCommit,
    ProviderRequestDelivery, ProviderRequestDeliveryOutcome, RoomSessionAuthorization, SqliteStore,
};
use agentsassemble_provider::{
    ProviderRequestCompletion, ProviderRequestExchange, ProviderRequestExchangeError,
    ProviderRequestResponder,
};
use chrono::Utc;
use futures_util::{StreamExt, stream::FuturesUnordered};
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

use super::RoomRuntime;

const MAX_LIVE_REQUESTS: usize = 64;
type Watch = Pin<Box<dyn Future<Output = RequestWake> + Send>>;

pub struct LiveProviderRequest {
    pub commit: ProviderRequestCommit,
    /// Exact open retries preserve the original recipient and return no new exchange.
    pub exchange: Option<ProviderRequestExchange>,
}

pub(super) enum OpeningAuthority {
    Managed { session_id: String },
    Attendee(AttendeeConnectionAuthorization),
}

pub(super) enum ResolutionAuthority {
    Session(RoomSessionAuthorization),
    Local {
        principal: AuthenticatedPrincipal,
        room_uid: Uuid,
    },
}

pub struct ResolvedProviderRequest {
    pub event: RoomEvent,
    pub deduplicated: bool,
}

pub(super) enum RequestCommand {
    Open {
        authority: OpeningAuthority,
        request: Box<OpenProviderRequest>,
        reply: oneshot::Sender<Result<LiveProviderRequest, PersistenceError>>,
    },
    Resolve {
        authority: Box<ResolutionAuthority>,
        request_id: Uuid,
        resolution: ProviderRequestResolution,
        reply: oneshot::Sender<Result<ResolvedProviderRequest, PersistenceError>>,
    },
}

struct Pending {
    session_id: String,
    generation: u64,
    execution_id: String,
    response: ProviderRequestResponder,
    delivery: Option<ProviderRequestDelivery>,
}

pub(super) struct RequestBroker {
    pending: HashMap<Uuid, Pending>,
    watches: FuturesUnordered<Watch>,
}

pub(super) struct RequestWake {
    request_id: Uuid,
    expired: bool,
    expires_at: chrono::DateTime<Utc>,
    delivered: bool,
    completion: ProviderRequestCompletion,
}

impl RequestBroker {
    pub fn new() -> Self {
        Self {
            pending: HashMap::new(),
            watches: FuturesUnordered::new(),
        }
    }

    pub fn has_watches(&self) -> bool {
        !self.watches.is_empty()
    }

    pub async fn wake(&mut self) -> Option<RequestWake> {
        self.watches.next().await
    }

    pub async fn apply(&mut self, store: &SqliteStore, room_id: &str, command: RequestCommand) {
        match command {
            RequestCommand::Open {
                authority,
                request,
                reply,
            } => {
                let result = self.open(store, room_id, authority, &request).await;
                // A lost reply drops the exchange, waking its watch for cancellation.
                let _ = reply.send(result);
            }
            RequestCommand::Resolve {
                authority,
                request_id,
                resolution,
                reply,
            } => {
                let result = self
                    .resolve(store, &authority, request_id, &resolution)
                    .await;
                let _ = reply.send(result);
            }
        }
    }

    async fn open(
        &mut self,
        store: &SqliteStore,
        room_id: &str,
        authority: OpeningAuthority,
        request: &OpenProviderRequest,
    ) -> Result<LiveProviderRequest, PersistenceError> {
        let id = request.request.provider_request_id;
        if self.pending.len() >= MAX_LIVE_REQUESTS && !self.pending.contains_key(&id) {
            return Err(unavailable(
                "provider_request_busy",
                "The live provider request capacity is full.",
            ));
        }
        let now = Utc::now();
        let commit = match authority {
            OpeningAuthority::Managed { session_id } => {
                store
                    .open_managed_provider_request(room_id, &session_id, request, now)
                    .await?
            }
            OpeningAuthority::Attendee(connection) => {
                store
                    .open_attendee_provider_request(
                        connection.session().session_fingerprint(),
                        connection.connection_id(),
                        request,
                        now,
                    )
                    .await?
            }
        };
        if self.pending.contains_key(&id) {
            return Ok(LiveProviderRequest {
                commit,
                exchange: None,
            });
        }
        if commit.deduplicated {
            return Err(unavailable(
                "provider_request_unavailable",
                "The original live request recipient is unavailable.",
            ));
        }
        let (exchange, response, mut completion) = ProviderRequestExchange::channel();
        let duration = (commit.expires_at - now).to_std().map_err(|_| {
            unavailable(
                "provider_request_closed",
                "The request deadline has elapsed.",
            )
        })?;
        let expires_at = commit.expires_at;
        let deadline = tokio::time::Instant::now() + duration;
        self.watches.push(Box::pin(async move {
            let (expired, delivered) = tokio::select! {
                () = tokio::time::sleep_until(deadline) => (true, false),
                delivered = completion.completion() => (false, delivered),
            };
            RequestWake {
                request_id: id,
                expires_at,
                expired,
                delivered,
                completion,
            }
        }));
        let session_id = commit.session_id.clone();
        self.pending.insert(
            id,
            Pending {
                session_id,
                generation: request.turn_generation,
                execution_id: request.execution_id.clone(),
                response,
                delivery: None,
            },
        );
        Ok(LiveProviderRequest {
            commit,
            exchange: Some(exchange),
        })
    }

    async fn resolve(
        &mut self,
        store: &SqliteStore,
        authority: &ResolutionAuthority,
        request_id: Uuid,
        resolution: &ProviderRequestResolution,
    ) -> Result<ResolvedProviderRequest, PersistenceError> {
        let principal = match authority {
            ResolutionAuthority::Local {
                principal,
                room_uid,
            } => {
                store
                    .require_room_incarnation(&principal.room_id, *room_uid)
                    .await?;
                store.resolve_principal(principal).await?
            }
            ResolutionAuthority::Session(session) => session.principal().clone(),
        };
        let authority = match authority {
            ResolutionAuthority::Local { .. } => {
                agentsassemble_persistence::RoomMutationAuthority::TrustedPrincipal(&principal)
            }
            ResolutionAuthority::Session(session) => session.mutation_authority(),
        };
        let commit = store
            .resolve_provider_request(authority, request_id, resolution, Utc::now())
            .await?;
        let deduplicated = commit.delivery.is_none();
        if let Some(delivery) = commit.delivery {
            let Some(pending) = self.pending.get_mut(&request_id) else {
                store
                    .complete_provider_request_delivery(
                        &delivery,
                        ProviderRequestDeliveryOutcome::Failed,
                        Utc::now(),
                    )
                    .await?;
                return Err(unavailable(
                    "provider_request_unavailable",
                    "The live request recipient is unavailable.",
                ));
            };
            if pending
                .response
                .respond(delivery.resolution().clone())
                .is_err()
            {
                pending.response.cancel();
                store
                    .complete_provider_request_delivery(
                        &delivery,
                        ProviderRequestDeliveryOutcome::Failed,
                        Utc::now(),
                    )
                    .await?;
                return Err(unavailable(
                    "provider_request_unavailable",
                    "The live request recipient has ended.",
                ));
            }
            pending.delivery = Some(delivery);
        }
        Ok(ResolvedProviderRequest {
            event: commit.event,
            deduplicated,
        })
    }

    pub async fn complete(
        &mut self,
        store: &SqliteStore,
        room_id: &str,
        wake: RequestWake,
    ) -> Result<(), PersistenceError> {
        let Some(pending) = self.pending.remove(&wake.request_id) else {
            wake.completion
                .finish(Err(ProviderRequestExchangeError::Closed));
            return Ok(());
        };
        let had_delivery = pending.delivery.is_some();
        let result = if wake.expired {
            store
                .expire_provider_request(room_id, wake.request_id, Utc::now().max(wake.expires_at))
                .await
                .map(|_| ())
        } else if let Some(delivery) = pending.delivery {
            let outcome = if wake.delivered {
                ProviderRequestDeliveryOutcome::Delivered
            } else {
                ProviderRequestDeliveryOutcome::Failed
            };
            store
                .complete_provider_request_delivery(&delivery, outcome, Utc::now())
                .await
                .map(|_| ())
        } else {
            store
                .cancel_provider_execution_requests(
                    room_id,
                    &pending.session_id,
                    pending.generation,
                    &pending.execution_id,
                )
                .await
                .map(|_| ())
        };
        wake.completion.finish(
            if result.is_ok() && had_delivery && wake.delivered && !wake.expired {
                Ok(())
            } else {
                Err(ProviderRequestExchangeError::DeliveryFailed)
            },
        );
        result
    }

    /// Called after real actor inputs, including publication wakes from external mutations.
    pub async fn reconcile(
        &mut self,
        store: &SqliteStore,
        room_id: &str,
    ) -> Result<(), PersistenceError> {
        if self.pending.is_empty() {
            return Ok(());
        }
        let durable: HashSet<_> = store
            .pending_provider_request_ids(room_id)
            .await?
            .into_iter()
            .collect();
        self.pending.retain(|id, pending| {
            if durable.contains(id) {
                true
            } else {
                pending.response.cancel();
                false
            }
        });
        Ok(())
    }
}

impl RoomRuntime {
    /// Opens a request from the managed runtime's exact active execution.
    ///
    /// # Errors
    /// Rejects queue saturation, wrong runtime custody and stale executions.
    pub async fn open_managed_request(
        &self,
        room_id: &str,
        session_id: &str,
        request: OpenProviderRequest,
    ) -> Result<LiveProviderRequest, PersistenceError> {
        self.open_provider_request(
            room_id,
            OpeningAuthority::Managed {
                session_id: session_id.to_owned(),
            },
            request,
        )
        .await
    }

    /// Opens a live request under the exact admitted socket authority.
    ///
    /// # Errors
    /// Rejects queue saturation, replaced custody and invalid durable request authority.
    pub async fn open_attendee_request(
        &self,
        connection: AttendeeConnectionAuthorization,
        request: OpenProviderRequest,
    ) -> Result<LiveProviderRequest, PersistenceError> {
        let room_id = connection.session().principal().room_id.clone();
        self.open_provider_request(&room_id, OpeningAuthority::Attendee(connection), request)
            .await
    }

    pub(super) async fn open_provider_request(
        &self,
        room_id: &str,
        authority: OpeningAuthority,
        request: OpenProviderRequest,
    ) -> Result<LiveProviderRequest, PersistenceError> {
        let (reply, response) = oneshot::channel();
        self.enqueue_request(
            room_id,
            RequestCommand::Open {
                authority,
                request: Box::new(request),
                reply,
            },
        )
        .await?;
        response.await.map_err(|_| {
            unavailable(
                "provider_request_unavailable",
                "The request open receipt was lost.",
            )
        })?
    }

    /// Resolves through the request owner without a generic command receipt containing secrets.
    ///
    /// # Errors
    /// Rejects invalid owner authority, changed answers and unavailable live recipients.
    pub async fn resolve_live_provider_request(
        &self,
        authority: RoomSessionAuthorization,
        request_id: Uuid,
        resolution: ProviderRequestResolution,
    ) -> Result<ResolvedProviderRequest, PersistenceError> {
        self.resolve_provider_response(
            ResolutionAuthority::Session(authority),
            request_id,
            resolution,
        )
        .await
    }

    pub(crate) async fn resolve_local_provider_request(
        &self,
        principal: AuthenticatedPrincipal,
        room_uid: Uuid,
        request_id: Uuid,
        resolution: ProviderRequestResolution,
    ) -> Result<ResolvedProviderRequest, PersistenceError> {
        self.resolve_provider_response(
            ResolutionAuthority::Local {
                principal,
                room_uid,
            },
            request_id,
            resolution,
        )
        .await
    }

    async fn resolve_provider_response(
        &self,
        authority: ResolutionAuthority,
        request_id: Uuid,
        resolution: ProviderRequestResolution,
    ) -> Result<ResolvedProviderRequest, PersistenceError> {
        let room_id = match &authority {
            ResolutionAuthority::Session(session) => session.principal().room_id.clone(),
            ResolutionAuthority::Local { principal, .. } => principal.room_id.clone(),
        };
        let (reply, response) = oneshot::channel();
        self.enqueue_request(
            &room_id,
            RequestCommand::Resolve {
                authority: Box::new(authority),
                request_id,
                resolution,
                reply,
            },
        )
        .await?;
        response.await.map_err(|_| {
            unavailable(
                "provider_request_unavailable",
                "The provider response receipt was lost.",
            )
        })?
    }

    async fn enqueue_request(
        &self,
        room_id: &str,
        command: RequestCommand,
    ) -> Result<(), PersistenceError> {
        self.handle(room_id)
            .await
            .provider_requests
            .try_send(command)
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => {
                    unavailable("room_busy", "The provider request queue is full.")
                }
                mpsc::error::TrySendError::Closed(_) => {
                    unavailable("room_unavailable", "The room request owner stopped.")
                }
            })
    }
}

fn unavailable(code: &'static str, message: &str) -> PersistenceError {
    PersistenceError::CommandRejected {
        code,
        message: message.to_owned(),
    }
}
