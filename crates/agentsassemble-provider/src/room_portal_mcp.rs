use std::{
    collections::VecDeque,
    convert::Infallible,
    sync::{Arc, Mutex},
    time::Duration,
};

use agentsassemble_domain::RoomRandomRequest;
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
use rmcp::{
    ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router,
    transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
    },
};
use serde_json::json;
use subtle::ConstantTimeEq;
use tokio::{
    net::TcpListener,
    sync::{OwnedSemaphorePermit, Semaphore},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::room_portal::{
    PortalState, RoomPortalError, StagedOutcome, canonical_message, reserve_room_tool,
    valid_decline_reason,
};
use crate::room_portal_tool_contract::{ChooseRandom, DeclineToSpeak, PublishMessage, RollDice};

const MAX_MCP_REQUEST_BYTES: usize = 64 * 1024;
const MAX_PORTAL_CONNECTIONS: usize = 8;
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
struct ConnectionRegistry {
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
enum ConnectionAuthentication {
    Authenticated,
    Unauthorized,
    Gone,
}

impl ConnectionRegistry {
    fn register(&self, abort: AbortHandle) -> u64 {
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

    fn authenticate(
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

    fn evict_oldest_unauthenticated(&self) -> Option<AbortHandle> {
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

#[derive(Debug, Clone)]
struct RoomPortalMcp {
    state: Arc<Mutex<PortalState>>,
    tool_router: ToolRouter<Self>,
}

impl RoomPortalMcp {
    fn new(state: Arc<Mutex<PortalState>>) -> Self {
        Self {
            state,
            tool_router: Self::tool_router(),
        }
    }
}

impl RoomPortalMcp {
    async fn execute_room_random(&self, request: RoomRandomRequest) -> Result<String, String> {
        let (authority, reservation, ingress) = reserve_room_tool(&self.state)?;
        let result = ingress
            .submit(authority, request, reservation)
            .await
            .map_err(|error| error.message)?;
        serde_json::to_string(&result)
            .map_err(|_| "The room tool result could not be encoded.".to_owned())
    }
}

#[tool_router]
impl RoomPortalMcp {
    #[tool(description = "Read the finalized messages in this turn's bounded shared-room view.")]
    fn read_discussion(&self) -> Result<String, String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "The shared room authority is unavailable.".to_owned())?;
        let active = state
            .active
            .as_mut()
            .ok_or_else(|| "No active room observation.".to_owned())?;
        active.receipt_generation = Some(active.turn_generation);
        Ok(active.room_view.clone())
    }

    #[tool(
        description = "Publish one substantive message to the shared room, optionally handing the floor to one exact agent ID. Read the discussion first."
    )]
    fn publish_message(
        &self,
        Parameters(input): Parameters<PublishMessage>,
    ) -> Result<String, String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "The shared room authority is unavailable.".to_owned())?;
        let active = state
            .active
            .as_mut()
            .ok_or_else(|| "No active room observation.".to_owned())?;
        let receipt_generation = active.turn_generation;
        if active.closing || active.outcome.is_some() {
            return Err("This turn already has a terminal room action.".to_owned());
        }
        if !active.tool_reservations.is_empty() {
            return Err("Wait for pending room tools before publishing.".to_owned());
        }
        let content = canonical_message(&input.content)
            .ok_or_else(|| "The room publication is invalid.".to_owned())?;
        let target_agent_id = if active
            .authority
            .allowed_agent_ids
            .contains(&input.next_agent_id)
        {
            input.next_agent_id
        } else {
            String::new()
        };
        active.outcome = Some(StagedOutcome::Message {
            receipt_generation,
            content,
            target_agent_id,
        });
        Ok("Published to the shared room.".to_owned())
    }

    #[tool(
        description = "End this room turn without posting, using one supported reason code: nothing_useful_to_add, not_addressed, or duplicate. Read the discussion first."
    )]
    fn decline_to_speak(
        &self,
        Parameters(input): Parameters<DeclineToSpeak>,
    ) -> Result<String, String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "The shared room authority is unavailable.".to_owned())?;
        let active = state
            .active
            .as_mut()
            .ok_or_else(|| "No active room observation.".to_owned())?;
        let receipt_generation = active.turn_generation;
        if active.closing || active.outcome.is_some() {
            return Err("This turn already has a terminal room action.".to_owned());
        }
        if !active.tool_reservations.is_empty() {
            return Err("Wait for pending room tools before declining.".to_owned());
        }
        if !valid_decline_reason(&input.reason_code) {
            return Err("The decline reason is unsupported.".to_owned());
        }
        active.outcome = Some(StagedOutcome::Declined {
            receipt_generation,
            reason_code: input.reason_code,
        });
        Ok("Declined this shared-room turn.".to_owned())
    }

    #[tool(
        description = "Roll bounded server-owned dice in tabletop mode. Read the discussion first."
    )]
    async fn roll_dice(&self, Parameters(input): Parameters<RollDice>) -> Result<String, String> {
        let request = RoomRandomRequest::parse(
            "room.random.roll",
            &json!({"notation": input.notation, "reason": input.reason}),
        )
        .map_err(|error| error.message)?;
        self.execute_room_random(request).await
    }

    #[tool(
        description = "Choose one bounded option with server-owned randomness in tabletop mode. Read the discussion first."
    )]
    async fn choose_random(
        &self,
        Parameters(input): Parameters<ChooseRandom>,
    ) -> Result<String, String> {
        let request = RoomRandomRequest::parse(
            "room.random.choose",
            &json!({"options": input.options, "reason": input.reason}),
        )
        .map_err(|error| error.message)?;
        self.execute_room_random(request).await
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for RoomPortalMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build()).with_instructions(
            "Read the bounded shared-room view, then publish once or decline once.",
        )
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeSet,
        sync::{Arc, Barrier},
        time::Duration,
    };

    use futures_util::future::AbortHandle;
    use rmcp::{
        ServiceExt,
        model::CallToolRequestParams,
        transport::{
            StreamableHttpClientTransport,
            streamable_http_client::StreamableHttpClientTransportConfig,
        },
    };
    use serde_json::{Map, json};

    use super::{
        ConnectionAuthentication, ConnectionRegistry, MAX_MCP_REQUEST_BYTES, MAX_PORTAL_CONNECTIONS,
    };
    use crate::room_portal::{ProviderTurnOutcome, RoomPortal};

    #[test]
    fn bearer_authentication_is_atomic_with_unauthenticated_eviction() {
        for _ in 0..128 {
            let registry = Arc::new(ConnectionRegistry::default());
            let (abort, _registration) = AbortHandle::new_pair();
            let connection_id = registry.register(abort);
            let barrier = Arc::new(Barrier::new(2));
            let (authentication, evicted) = std::thread::scope(|scope| {
                let authentication_registry = registry.clone();
                let authentication_barrier = barrier.clone();
                let authentication = scope.spawn(move || {
                    authentication_barrier.wait();
                    authentication_registry.authenticate(
                        connection_id,
                        Some(b"Bearer portal-token"),
                        b"Bearer portal-token",
                    )
                });
                let eviction_registry = registry.clone();
                let eviction = scope.spawn(move || {
                    barrier.wait();
                    eviction_registry.evict_oldest_unauthenticated()
                });
                (
                    authentication
                        .join()
                        .unwrap_or_else(|_| panic!("join authentication race")),
                    eviction
                        .join()
                        .unwrap_or_else(|_| panic!("join eviction race")),
                )
            });
            match authentication {
                ConnectionAuthentication::Authenticated => assert!(evicted.is_none()),
                ConnectionAuthentication::Gone => assert!(evicted.is_some()),
                ConnectionAuthentication::Unauthorized => {
                    panic!("the exact bearer was rejected")
                }
            }
        }
    }

    #[tokio::test]
    async fn loopback_mcp_requires_a_same_turn_read_before_commit() {
        let portal = RoomPortal::create()
            .await
            .unwrap_or_else(|error| panic!("create room portal fixture: {error}"));
        portal
            .begin_observation(
                "agent-1",
                "turn-1",
                7,
                "Room: General\n#7 Human: hello",
                &["agent-2".to_owned()],
                None,
            )
            .unwrap_or_else(|error| panic!("begin room observation: {error}"));
        let client = ()
            .serve(StreamableHttpClientTransport::from_config(
                StreamableHttpClientTransportConfig::with_uri(portal.endpoint())
                    .auth_header(portal.bearer_token()),
            ))
            .await
            .unwrap_or_else(|error| panic!("connect room portal MCP: {error}"));
        let names = client
            .list_all_tools()
            .await
            .unwrap_or_else(|error| panic!("list room portal tools: {error}"))
            .into_iter()
            .map(|tool| tool.name.to_string())
            .collect::<BTreeSet<_>>();
        assert_eq!(
            names,
            BTreeSet::from([
                "choose_random".to_owned(),
                "decline_to_speak".to_owned(),
                "publish_message".to_owned(),
                "read_discussion".to_owned(),
                "roll_dice".to_owned(),
            ])
        );
        let early = call_tool(
            &client,
            "publish_message",
            json!({"content": "  canonical reply  ", "next_agent_id": "unknown"}),
        )
        .await;
        assert_ne!(early.is_error, Some(true));
        assert!(portal.finish_observation("turn-1", 7).is_err());
        let read = call_tool(&client, "read_discussion", json!({})).await;
        let read_text = read
            .content
            .first()
            .and_then(|content| content.as_text())
            .map(|content| content.text.as_str())
            .unwrap_or_default();
        assert!(read_text.contains("Human: hello"));
        let duplicate = call_tool(
            &client,
            "decline_to_speak",
            json!({"reason_code": "duplicate"}),
        )
        .await;
        assert_eq!(duplicate.is_error, Some(true));
        assert_eq!(
            portal
                .finish_observation("turn-1", 7)
                .unwrap_or_else(|error| panic!("finish room observation: {error}")),
            ProviderTurnOutcome::Message {
                content: "canonical reply".to_owned(),
                target_agent_id: String::new(),
            }
        );
        let _ = client.cancel().await;
    }

    #[tokio::test]
    async fn loopback_mcp_hides_capability_path_and_bounds_request_bodies() {
        let portal = RoomPortal::create()
            .await
            .unwrap_or_else(|error| panic!("create bounded portal fixture: {error}"));
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap_or_else(|error| panic!("build portal HTTP client: {error}"));
        let endpoint = reqwest::Url::parse(portal.endpoint())
            .unwrap_or_else(|error| panic!("parse portal endpoint: {error}"));
        let root = format!(
            "{}://{}:{}/",
            endpoint.scheme(),
            endpoint.host_str().unwrap_or("127.0.0.1"),
            endpoint.port().unwrap_or_default()
        );
        let address = format!(
            "{}:{}",
            endpoint.host_str().unwrap_or("127.0.0.1"),
            endpoint.port().unwrap_or_default()
        );
        let mut idle_connections = Vec::new();
        for _ in 0..16 {
            idle_connections.push(
                tokio::net::TcpStream::connect(&address)
                    .await
                    .unwrap_or_else(|error| panic!("open idle loopback connection: {error}")),
            );
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(portal.active_connection_count() <= MAX_PORTAL_CONNECTIONS);
        let admitted = client
            .post(portal.endpoint())
            .bearer_auth(portal.bearer_token())
            .header("accept", "application/json, text/event-stream")
            .header("content-type", "application/json")
            .body("{}")
            .send()
            .await
            .unwrap_or_else(|error| panic!("probe authenticated admission: {error}"));
        assert_ne!(admitted.status(), reqwest::StatusCode::SERVICE_UNAVAILABLE);
        assert_ne!(admitted.status(), reqwest::StatusCode::UNAUTHORIZED);
        let hidden = client
            .post(root)
            .body("{}")
            .send()
            .await
            .unwrap_or_else(|error| panic!("probe hidden portal route: {error}"));
        assert_eq!(hidden.status(), reqwest::StatusCode::NOT_FOUND);
        let unauthorized = client
            .post(portal.endpoint())
            .header("accept", "application/json, text/event-stream")
            .header("content-type", "application/json")
            .body("{}")
            .send()
            .await
            .unwrap_or_else(|error| panic!("probe portal authorization: {error}"));
        assert_eq!(unauthorized.status(), reqwest::StatusCode::UNAUTHORIZED);
        let oversized = client
            .post(portal.endpoint())
            .bearer_auth(portal.bearer_token())
            .header("accept", "application/json, text/event-stream")
            .header("content-type", "application/json")
            .body("x".repeat(MAX_MCP_REQUEST_BYTES + 1))
            .send()
            .await
            .unwrap_or_else(|error| panic!("send oversized MCP body: {error}"));
        assert_eq!(oversized.status(), reqwest::StatusCode::PAYLOAD_TOO_LARGE);
        drop(idle_connections);
    }

    async fn call_tool(
        client: &rmcp::service::RunningService<rmcp::RoleClient, ()>,
        name: &'static str,
        arguments: serde_json::Value,
    ) -> rmcp::model::CallToolResult {
        let arguments = serde_json::from_value::<Map<String, serde_json::Value>>(arguments)
            .unwrap_or_else(|error| panic!("decode tool arguments: {error}"));
        client
            .call_tool(CallToolRequestParams::new(name).with_arguments(arguments))
            .await
            .unwrap_or_else(|error| panic!("call {name}: {error}"))
    }
}
