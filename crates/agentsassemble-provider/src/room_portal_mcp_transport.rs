use std::{
    collections::VecDeque,
    convert::Infallible,
    sync::{Arc, Mutex},
    time::Duration,
};

use bytes::Bytes;
use futures_util::future::{AbortHandle, Abortable};
use http_body_util::{BodyExt, Empty, combinators::BoxBody};
use hyper::{
    Request, Response, StatusCode,
    body::Incoming,
    header::{AUTHORIZATION, CONNECTION, WWW_AUTHENTICATE},
    server::conn::http1,
    service::service_fn,
};
use hyper_util::rt::{TokioIo, TokioTimer};
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use subtle::ConstantTimeEq;
use tokio::{
    net::TcpListener,
    sync::{OwnedSemaphorePermit, Semaphore},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::{
    room_portal::{PortalState, RoomPortalError},
    room_portal_mcp::RoomPortalMcp,
};

pub(super) const MAX_MCP_REQUEST_BYTES: usize = 64 * 1024;
pub(super) const MAX_PORTAL_CONNECTIONS: usize = 8;
const MAX_PORTAL_REQUESTS: usize = 8;
const PORTAL_HEADER_TIMEOUT: Duration = Duration::from_secs(2);
const PORTAL_CONNECTION_TIMEOUT: Duration = Duration::from_secs(5);
const PORTAL_REQUEST_TIMEOUT: Duration = Duration::from_secs(3);
const PORTAL_EVICTION_TIMEOUT: Duration = Duration::from_millis(100);

type PortalHttpService = StreamableHttpService<RoomPortalMcp, LocalSessionManager>;
type PortalBody = BoxBody<Bytes, Infallible>;

pub(super) struct PortalServer {
    endpoint: String,
    bearer_token: String,
    cancellation: CancellationToken,
    task: JoinHandle<()>,
    #[cfg(test)]
    connections: Arc<ConnectionRegistry>,
}

impl PortalServer {
    pub(super) async fn start(state: Arc<Mutex<PortalState>>) -> Result<Self, RoomPortalError> {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|_| RoomPortalError::Mcp)?;
        let address = listener.local_addr().map_err(|_| RoomPortalError::Mcp)?;
        let capability_path = format!("/portal/{}/mcp", Uuid::new_v4().simple());
        let endpoint = format!("http://{address}{capability_path}");
        let bearer_token = Uuid::new_v4().simple().to_string();
        let cancellation = CancellationToken::new();
        let service = StreamableHttpService::new(
            move || Ok(RoomPortalMcp::new(state.clone())),
            Arc::<LocalSessionManager>::default(),
            StreamableHttpServerConfig::default()
                .with_legacy_session_mode(false)
                .with_json_response(true)
                .with_sse_keep_alive(None)
                .with_allowed_hosts([address.to_string()])
                .with_allowed_origins([format!("http://{address}")])
                .with_max_request_body_bytes(MAX_MCP_REQUEST_BYTES)
                .with_cancellation_token(cancellation.child_token()),
        );
        let task_cancellation = cancellation.clone();
        let connections = Arc::new(ConnectionRegistry::default());
        let task = tokio::spawn(run_server(
            listener,
            capability_path,
            format!("Bearer {bearer_token}"),
            service,
            connections.clone(),
            task_cancellation,
        ));
        Ok(Self {
            endpoint,
            bearer_token,
            cancellation,
            task,
            #[cfg(test)]
            connections,
        })
    }

    pub(super) fn endpoint(&self) -> &str {
        &self.endpoint
    }

    pub(super) fn bearer_token(&self) -> &str {
        &self.bearer_token
    }

    pub(super) fn is_running(&self) -> bool {
        !self.cancellation.is_cancelled() && !self.task.is_finished()
    }

    #[cfg(test)]
    pub(crate) fn active_connection_count(&self) -> usize {
        self.connections.active_count()
    }
}

impl Drop for PortalServer {
    fn drop(&mut self) {
        self.cancellation.cancel();
        self.task.abort();
    }
}

async fn run_server(
    listener: TcpListener,
    capability_path: String,
    expected_authorization: String,
    service: PortalHttpService,
    connections: Arc<ConnectionRegistry>,
    cancellation: CancellationToken,
) {
    let connection_admission = Arc::new(Semaphore::new(MAX_PORTAL_CONNECTIONS));
    let request_admission = Arc::new(Semaphore::new(MAX_PORTAL_REQUESTS));
    loop {
        let accepted = tokio::select! {
            () = cancellation.cancelled() => return,
            accepted = listener.accept() => accepted,
        };
        let Ok((stream, peer)) = accepted else {
            return;
        };
        if !peer.ip().is_loopback() {
            continue;
        }
        let Some(permit) = admit_connection(
            connection_admission.clone(),
            connections.clone(),
            &cancellation,
        )
        .await
        else {
            continue;
        };
        let (abort_handle, abort_registration) = AbortHandle::new_pair();
        let connection_id = connections.register(abort_handle);
        let connection_lease = ConnectionLease {
            id: connection_id,
            registry: connections.clone(),
        };
        let service = service.clone();
        let expected_path = capability_path.clone();
        let expected_authorization = expected_authorization.clone();
        let request_admission = request_admission.clone();
        let request_connections = connections.clone();
        let connection_cancellation = cancellation.child_token();
        tokio::spawn(async move {
            let connection = async move {
                let _permit = permit;
                let _lease = connection_lease;
                let io = TokioIo::new(stream);
                let http_service = service_fn(move |request| {
                    serve_request(
                        request,
                        service.clone(),
                        expected_path.clone(),
                        expected_authorization.clone(),
                        request_admission.clone(),
                        request_connections.clone(),
                        connection_id,
                    )
                });
                let mut builder = http1::Builder::new();
                builder
                    .timer(TokioTimer::new())
                    .header_read_timeout(PORTAL_HEADER_TIMEOUT)
                    .keep_alive(false);
                let connection = builder.serve_connection(io, http_service);
                tokio::pin!(connection);
                tokio::select! {
                    () = connection_cancellation.cancelled() => {}
                    () = tokio::time::sleep(PORTAL_CONNECTION_TIMEOUT) => {}
                    _ = &mut connection => {}
                }
            };
            let _ = Abortable::new(connection, abort_registration).await;
        });
    }
}

async fn admit_connection(
    admission: Arc<Semaphore>,
    connections: Arc<ConnectionRegistry>,
    cancellation: &CancellationToken,
) -> Option<OwnedSemaphorePermit> {
    if let Ok(permit) = admission.clone().try_acquire_owned() {
        return Some(permit);
    }
    connections.evict_oldest_unauthenticated()?.abort();
    tokio::select! {
        () = cancellation.cancelled() => None,
        permit = tokio::time::timeout(PORTAL_EVICTION_TIMEOUT, admission.acquire_owned()) => {
            permit.ok()?.ok()
        }
    }
}

async fn serve_request(
    request: Request<Incoming>,
    service: PortalHttpService,
    expected_path: String,
    expected_authorization: String,
    admission: Arc<Semaphore>,
    connections: Arc<ConnectionRegistry>,
    connection_id: u64,
) -> Result<Response<PortalBody>, Infallible> {
    if request.uri().path() != expected_path || request.uri().query().is_some() {
        return Ok(empty_response(StatusCode::NOT_FOUND));
    }
    match connections.authenticate(
        connection_id,
        request
            .headers()
            .get(AUTHORIZATION)
            .map(hyper::header::HeaderValue::as_bytes),
        expected_authorization.as_bytes(),
    ) {
        ConnectionAuthentication::Authenticated => {}
        ConnectionAuthentication::Unauthorized => {
            let mut response = empty_response(StatusCode::UNAUTHORIZED);
            response.headers_mut().insert(
                WWW_AUTHENTICATE,
                hyper::header::HeaderValue::from_static("Bearer"),
            );
            return Ok(response);
        }
        ConnectionAuthentication::Gone => {
            return Ok(empty_response(StatusCode::SERVICE_UNAVAILABLE));
        }
    }
    let Ok(_permit) = admission.try_acquire_owned() else {
        return Ok(empty_response(StatusCode::SERVICE_UNAVAILABLE));
    };
    match tokio::time::timeout(PORTAL_REQUEST_TIMEOUT, service.handle(request)).await {
        Ok(response) => Ok(with_connection_close(response)),
        Err(_) => Ok(empty_response(StatusCode::REQUEST_TIMEOUT)),
    }
}

#[derive(Default)]
pub(super) struct ConnectionRegistry {
    state: Mutex<ConnectionRegistryState>,
}

#[derive(Default)]
struct ConnectionRegistryState {
    next_id: u64,
    active: VecDeque<ActiveConnection>,
}

struct ActiveConnection {
    id: u64,
    authenticated: bool,
    abort: AbortHandle,
}

#[derive(Debug, Eq, PartialEq)]
pub(super) enum ConnectionAuthentication {
    Authenticated,
    Unauthorized,
    Gone,
}

impl ConnectionRegistry {
    pub(super) fn register(&self, abort: AbortHandle) -> u64 {
        let Ok(mut state) = self.state.lock() else {
            abort.abort();
            return 0;
        };
        state.next_id = state.next_id.saturating_add(1).max(1);
        let id = state.next_id;
        state.active.push_back(ActiveConnection {
            id,
            authenticated: false,
            abort,
        });
        id
    }

    pub(super) fn authenticate(
        &self,
        id: u64,
        observed_authorization: Option<&[u8]>,
        expected_authorization: &[u8],
    ) -> ConnectionAuthentication {
        let Ok(mut state) = self.state.lock() else {
            return ConnectionAuthentication::Gone;
        };
        let authorized = observed_authorization.is_some_and(|observed| {
            observed.len() == expected_authorization.len()
                && bool::from(observed.ct_eq(expected_authorization))
        });
        if !authorized {
            return ConnectionAuthentication::Unauthorized;
        }
        let Some(connection) = state.active.iter_mut().find(|entry| entry.id == id) else {
            return ConnectionAuthentication::Gone;
        };
        connection.authenticated = true;
        ConnectionAuthentication::Authenticated
    }

    pub(super) fn evict_oldest_unauthenticated(&self) -> Option<AbortHandle> {
        let mut state = self.state.lock().ok()?;
        let index = state
            .active
            .iter()
            .position(|connection| !connection.authenticated)?;
        state
            .active
            .remove(index)
            .map(|connection| connection.abort)
    }

    fn remove(&self, id: u64) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        if let Some(index) = state.active.iter().position(|entry| entry.id == id) {
            state.active.remove(index);
        }
    }

    #[cfg(test)]
    fn active_count(&self) -> usize {
        self.state
            .lock()
            .map_or(usize::MAX, |state| state.active.len())
    }
}

struct ConnectionLease {
    id: u64,
    registry: Arc<ConnectionRegistry>,
}

impl Drop for ConnectionLease {
    fn drop(&mut self) {
        self.registry.remove(self.id);
    }
}

fn empty_response(status: StatusCode) -> Response<PortalBody> {
    let mut response = Response::new(Empty::<Bytes>::new().boxed());
    *response.status_mut() = status;
    with_connection_close(response)
}

fn with_connection_close(mut response: Response<PortalBody>) -> Response<PortalBody> {
    response
        .headers_mut()
        .insert(CONNECTION, hyper::header::HeaderValue::from_static("close"));
    response
}
