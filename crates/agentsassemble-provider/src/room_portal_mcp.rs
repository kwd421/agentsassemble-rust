use std::{
    convert::Infallible,
    sync::{Arc, Mutex},
    time::Duration,
};

use bytes::Bytes;
use http_body_util::{BodyExt, Empty, combinators::BoxBody};
use hyper::{
    Request, Response, StatusCode, body::Incoming, server::conn::http1, service::service_fn,
};
use hyper_util::rt::{TokioIo, TokioTimer};
use rmcp::{
    ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{ServerCapabilities, ServerInfo},
    schemars, tool, tool_handler, tool_router,
    transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
    },
};
use serde::Deserialize;
use tokio::{net::TcpListener, sync::Semaphore, task::JoinHandle};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::room_portal::{
    PortalState, RoomPortalError, StagedOutcome, canonical_message, valid_decline_reason,
};

const MAX_MCP_REQUEST_BYTES: usize = 64 * 1024;
const MAX_PORTAL_CONNECTIONS: usize = 8;
const MAX_PORTAL_REQUESTS: usize = 8;
const PORTAL_HEADER_TIMEOUT: Duration = Duration::from_secs(2);

type PortalHttpService = StreamableHttpService<RoomPortalMcp, LocalSessionManager>;
type PortalBody = BoxBody<Bytes, Infallible>;

pub(super) struct PortalServer {
    endpoint: String,
    cancellation: CancellationToken,
    task: JoinHandle<()>,
}

impl PortalServer {
    pub(super) async fn start(state: Arc<Mutex<PortalState>>) -> Result<Self, RoomPortalError> {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|_| RoomPortalError::Mcp)?;
        let address = listener.local_addr().map_err(|_| RoomPortalError::Mcp)?;
        let capability_path = format!("/portal/{}/mcp", Uuid::new_v4().simple());
        let endpoint = format!("http://{address}{capability_path}");
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
        let task = tokio::spawn(run_server(
            listener,
            capability_path,
            service,
            task_cancellation,
        ));
        Ok(Self {
            endpoint,
            cancellation,
            task,
        })
    }

    pub(super) fn endpoint(&self) -> &str {
        &self.endpoint
    }

    pub(super) fn is_running(&self) -> bool {
        !self.cancellation.is_cancelled() && !self.task.is_finished()
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
    service: PortalHttpService,
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
        let Ok(connection_permit) = connection_admission.clone().try_acquire_owned() else {
            continue;
        };
        let service = service.clone();
        let expected_path = capability_path.clone();
        let request_admission = request_admission.clone();
        let connection_cancellation = cancellation.child_token();
        tokio::spawn(async move {
            let io = TokioIo::new(stream);
            let http_service = service_fn(move |request| {
                serve_request(
                    request,
                    service.clone(),
                    expected_path.clone(),
                    request_admission.clone(),
                )
            });
            let mut builder = http1::Builder::new();
            builder
                .timer(TokioTimer::new())
                .header_read_timeout(PORTAL_HEADER_TIMEOUT);
            let connection = builder.serve_connection(io, http_service);
            tokio::pin!(connection);
            tokio::select! {
                () = connection_cancellation.cancelled() => {}
                _ = &mut connection => {}
            }
            drop(connection_permit);
        });
    }
}

async fn serve_request(
    request: Request<Incoming>,
    service: PortalHttpService,
    expected_path: String,
    admission: Arc<Semaphore>,
) -> Result<Response<PortalBody>, Infallible> {
    if request.uri().path() != expected_path || request.uri().query().is_some() {
        return Ok(empty_response(StatusCode::NOT_FOUND));
    }
    let Ok(_permit) = admission.try_acquire_owned() else {
        return Ok(empty_response(StatusCode::SERVICE_UNAVAILABLE));
    };
    Ok(service.handle(request).await)
}

fn empty_response(status: StatusCode) -> Response<PortalBody> {
    let mut response = Response::new(Empty::<Bytes>::new().boxed());
    *response.status_mut() = status;
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

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct PublishMessage {
    content: String,
    #[serde(default)]
    next_agent_id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct DeclineToSpeak {
    reason_code: String,
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
        if active.receipt_generation.is_none() {
            active.receipt_generation = Some(Uuid::new_v4());
        }
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
        let receipt_generation = active
            .receipt_generation
            .ok_or_else(|| "Read the shared room discussion before publishing.".to_owned())?;
        if active.outcome.is_some() {
            return Err("This turn already has a terminal room action.".to_owned());
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
        let receipt_generation = active
            .receipt_generation
            .ok_or_else(|| "Read the shared room discussion before declining.".to_owned())?;
        if active.outcome.is_some() {
            return Err("This turn already has a terminal room action.".to_owned());
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
    use std::{collections::BTreeSet, time::Duration};

    use rmcp::{
        ServiceExt, model::CallToolRequestParams, transport::StreamableHttpClientTransport,
    };
    use serde_json::{Map, json};

    use super::MAX_MCP_REQUEST_BYTES;
    use crate::room_portal::{ProviderTurnOutcome, RoomPortal};

    #[tokio::test]
    async fn loopback_mcp_enforces_read_before_one_publication() {
        let portal = RoomPortal::create()
            .await
            .unwrap_or_else(|error| panic!("create room portal fixture: {error}"));
        portal
            .begin_observation(
                "turn-1",
                7,
                "Room: General\n#7 Human: hello",
                &["agent-2".to_owned()],
            )
            .unwrap_or_else(|error| panic!("begin room observation: {error}"));
        let client = ()
            .serve(StreamableHttpClientTransport::from_uri(portal.endpoint()))
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
                "decline_to_speak".to_owned(),
                "publish_message".to_owned(),
                "read_discussion".to_owned(),
            ])
        );
        let early = call_tool(
            &client,
            "publish_message",
            json!({"content": "too early", "next_agent_id": "agent-2"}),
        )
        .await;
        assert_eq!(early.is_error, Some(true));
        assert!(portal.finish_observation("turn-1", 7).is_err());
        let read = call_tool(&client, "read_discussion", json!({})).await;
        let read_text = read
            .content
            .first()
            .and_then(|content| content.as_text())
            .map(|content| content.text.as_str())
            .unwrap_or_default();
        assert!(read_text.contains("Human: hello"));
        let publish = call_tool(
            &client,
            "publish_message",
            json!({"content": "  canonical reply  ", "next_agent_id": "unknown"}),
        )
        .await;
        assert_ne!(publish.is_error, Some(true));
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
        let hidden = client
            .post(root)
            .body("{}")
            .send()
            .await
            .unwrap_or_else(|error| panic!("probe hidden portal route: {error}"));
        assert_eq!(hidden.status(), reqwest::StatusCode::NOT_FOUND);
        let oversized = client
            .post(portal.endpoint())
            .header("accept", "application/json, text/event-stream")
            .header("content-type", "application/json")
            .body("x".repeat(MAX_MCP_REQUEST_BYTES + 1))
            .send()
            .await
            .unwrap_or_else(|error| panic!("send oversized MCP body: {error}"));
        assert_eq!(oversized.status(), reqwest::StatusCode::PAYLOAD_TOO_LARGE);
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
