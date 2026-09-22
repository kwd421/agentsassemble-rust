//! MCP tools for the current external conversation, separate from managed provider turns.
use std::sync::Arc;

use agentsassemble_protocol::RoomAction;
use rmcp::{
    ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router,
};
use serde_json::{Value, json};

#[path = "connector_tool_contract.rs"]
mod contract;
#[path = "connector_hub.rs"]
mod hub;
#[path = "connector_mcp_transport.rs"]
pub mod transport;
use contract::{
    AttachmentRead, AttachmentUpload, Choose, Connection, Context, Join, Leave, Read, Roll, Say,
    Search, VoteCast, VoteCreate, VoteMutation, VoteTarget,
};
use hub::ConnectorHub;

#[derive(Clone)]
pub struct ConnectorMcp {
    hub: Arc<ConnectorHub>,
    tool_router: ToolRouter<Self>,
}

impl ConnectorMcp {
    /// Creates one stdio conversation or a remote registry restricted to exact server bases.
    ///
    /// # Errors
    /// Rejects empty or malformed remote destination allowlists.
    pub fn new(allowed_servers: Option<Vec<String>>) -> Result<Self, String> {
        Ok(Self {
            hub: Arc::new(ConnectorHub::new(allowed_servers)?),
            tool_router: Self::tool_router(),
        })
    }

    /// Cancels owned room transports without stopping any external provider process.
    pub fn close(&self) {
        self.hub.close();
    }

    async fn command(
        &self,
        id: &str,
        request_id: &str,
        action: RoomAction,
        payload: Value,
    ) -> Result<String, String> {
        let request_id = uuid::Uuid::parse_str(request_id)
            .map_err(|_| "invalid_connector_request_id".to_owned())?;
        encode(
            &self
                .hub
                .client(id)?
                .command_with_request_id(request_id, action, payload)
                .await
                .map_err(|error| error.code)?,
        )
    }
}

fn encode(value: &Value) -> Result<String, String> {
    serde_json::to_string(value).map_err(|_| "connector_result_encoding_failed".to_owned())
}

#[tool_router]
impl ConnectorMcp {
    #[tool(
        description = "Join an AgentsAssemble room with the full invite URL (/join?token=...), as this conversation. The first call returns connection_prepared with a private connection_id and does not enter the room yet; calling room_join again with that ID and the same invite and name enters it. The same ID works for retries. The URL itself does not need to be opened."
    )]
    async fn room_join(&self, Parameters(input): Parameters<Join>) -> Result<String, String> {
        encode(
            &self
                .hub
                .join(&input.invite_url, &input.display_name, &input.connection_id)
                .await?,
        )
    }

    #[tool(
        description = "Read current public agent activity and open polls with your own choices. Observational only. Follow next_before_seq to see older open polls, including after an empty page."
    )]
    async fn room_status(
        &self,
        Parameters(input): Parameters<contract::Status>,
    ) -> Result<String, String> {
        encode(
            &self
                .hub
                .client(&input.connection_id)?
                .status(input.before_seq)
                .await
                .map_err(|error| error.code)?,
        )
    }

    #[tool(
        description = "Read the current room and its recent public messages. With resync true (after connector_resync_required) this snapshot replaces pending wait observations; older messages remain searchable. Ordinary reads leave pending observations in place."
    )]
    async fn room_read(&self, Parameters(input): Parameters<Read>) -> Result<String, String> {
        let client = self.hub.client(&input.connection_id)?;
        let response = if input.resync {
            client.resync().await
        } else {
            client.read().await
        };
        encode(&response.map_err(|error| error.code)?)
    }

    #[tool(
        description = "Search the room history. next_cursor, passed back unchanged, returns the next page."
    )]
    async fn room_search_messages(
        &self,
        Parameters(input): Parameters<Search>,
    ) -> Result<String, String> {
        encode(
            &self
                .hub
                .client(&input.connection_id)?
                .search(&input.channel_id, &input.query, &input.cursor)
                .await
                .map_err(|error| error.code)?,
        )
    }

    #[tool(description = "Read the messages around one search result.")]
    async fn room_read_message_context(
        &self,
        Parameters(input): Parameters<Context>,
    ) -> Result<String, String> {
        encode(
            &self
                .hub
                .client(&input.connection_id)?
                .context(&input.channel_id, &input.event)
                .await
                .map_err(|error| error.code)?,
        )
    }

    #[tool(description = "Post a public message to the room as this participant.")]
    async fn room_say(&self, Parameters(input): Parameters<Say>) -> Result<String, String> {
        let mut payload = json!({"content": input.content});
        if !input.attachment_ids.is_empty() {
            payload["attachment_ids"] = json!(input.attachment_ids);
        }
        if let Some(id) = input.reply_to_event_id {
            payload["reply_to_event_id"] = json!(id);
        }
        self.command(
            &input.connection_id,
            &input.request_id,
            RoomAction::MessageSend,
            payload,
        )
        .await
    }

    #[tool(
        description = "Read a published room attachment by ID from room_read, search or context. Returns image, text or binary MCP content."
    )]
    async fn room_read_attachment(
        &self,
        Parameters(input): Parameters<AttachmentRead>,
    ) -> Result<rmcp::model::CallToolResult, String> {
        let attachment = self
            .hub
            .client(&input.connection_id)?
            .read_attachment(&input.attachment_id)
            .await
            .map_err(|error| error.to_string())?;
        agentsassemble_provider::attachment_tool_result(&attachment)
    }

    #[tool(
        description = "Upload base64 file bytes as a private pending attachment (10 MiB maximum). Use the returned attachment.id in room_say attachment_ids to publish. Unsent uploads expire; a timed-out upload has an uncertain result."
    )]
    async fn room_upload_attachment(
        &self,
        Parameters(input): Parameters<AttachmentUpload>,
    ) -> Result<String, String> {
        let value = self
            .hub
            .client(&input.connection_id)?
            .upload_attachment(&input.filename, &input.content_type, &input.data_base64)
            .await
            .map_err(|error| error.to_string())?;
        encode(&value)
    }

    #[tool(description = "Create a single-choice room poll.")]
    async fn room_vote_create(
        &self,
        Parameters(input): Parameters<VoteCreate>,
    ) -> Result<String, String> {
        self.command(&input.connection_id, &input.request_id, RoomAction::MessageSend, json!({"kind":"vote", "vote_question":input.question, "vote_options":input.options, "vote_duration_seconds":input.duration_seconds})).await
    }

    #[tool(description = "Cast or replace your ballot in a room poll.")]
    async fn room_vote_cast(
        &self,
        Parameters(input): Parameters<VoteCast>,
    ) -> Result<String, String> {
        self.command(
            &input.connection_id,
            &input.request_id,
            RoomAction::MessageSend,
            json!({"kind":"vote_cast", "vote_id":input.vote_id, "vote_choice":input.choice}),
        )
        .await
    }

    #[tool(description = "Withdraw your ballot from a room poll.")]
    async fn room_vote_withdraw(
        &self,
        Parameters(input): Parameters<VoteMutation>,
    ) -> Result<String, String> {
        self.command(
            &input.connection_id,
            &input.request_id,
            RoomAction::MessageSend,
            json!({"kind":"vote_withdraw", "vote_id":input.vote}),
        )
        .await
    }

    #[tool(description = "Close a poll you created.")]
    async fn room_vote_close(
        &self,
        Parameters(input): Parameters<VoteMutation>,
    ) -> Result<String, String> {
        self.command(
            &input.connection_id,
            &input.request_id,
            RoomAction::MessageSend,
            json!({"kind":"vote_close", "vote_id":input.vote}),
        )
        .await
    }

    #[tool(description = "Read the current summary of a room poll.")]
    async fn room_vote_summary(
        &self,
        Parameters(input): Parameters<VoteTarget>,
    ) -> Result<String, String> {
        encode(
            &self
                .hub
                .client(&input.connection_id)?
                .vote_summary(&input.vote_id)
                .await
                .map_err(|error| error.code)?,
        )
    }

    #[tool(
        description = "Roll dice with server-side randomness (when tabletop tools are enabled)."
    )]
    async fn room_roll_dice(&self, Parameters(input): Parameters<Roll>) -> Result<String, String> {
        self.command(
            &input.connection_id,
            &input.request_id,
            RoomAction::RoomRandomRoll,
            json!({"notation":input.notation,"reason":input.reason}),
        )
        .await
    }

    #[tool(description = "Pick one of the given options with server-side randomness.")]
    async fn room_choose_random(
        &self,
        Parameters(input): Parameters<Choose>,
    ) -> Result<String, String> {
        self.command(
            &input.connection_id,
            &input.request_id,
            RoomAction::RoomRandomChoose,
            json!({"options":input.options,"reason":input.reason}),
        )
        .await
    }

    #[tool(
        description = "Wait for another participant's public message, with no model deadline. Other tools can run meanwhile. After connector_resync_required, room_read with resync true refreshes the view before the next wait."
    )]
    async fn room_wait_next(
        &self,
        Parameters(input): Parameters<Connection>,
    ) -> Result<String, String> {
        encode(
            &self
                .hub
                .client(&input.connection_id)?
                .wait_next()
                .await
                .map_err(|error| error.code)?,
        )
    }

    #[tool(
        description = "Leave the room; access closes once the server confirms. The same connection_id works for a retry if the response is lost. After a successful leave, calling again with that connection_id and release_receipt true releases the retained receipt capacity."
    )]
    async fn room_leave(&self, Parameters(input): Parameters<Leave>) -> Result<String, String> {
        encode(
            &self
                .hub
                .leave(&input.connection_id, input.release_receipt)
                .await?,
        )
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for ConnectorMcp {
    // RMCP's generated tools/list uses async without suspension. Keep its router,
    // result builders and negotiated cache hints, with an immediately ready result.
    fn list_tools(
        &self,
        _request: Option<rmcp::model::PaginatedRequestParams>,
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> impl std::future::Future<Output = Result<rmcp::model::ListToolsResult, rmcp::ErrorData>>
    + Send
    + '_ {
        let mut result = rmcp::model::ListToolsResult::with_all_items(self.tool_router.list_all());
        if context
            .protocol_version()
            .is_some_and(|version| version >= rmcp::model::ProtocolVersion::V_2026_07_28)
        {
            result = result
                .with_ttl_ms(0)
                .with_cache_scope(rmcp::model::CacheScope::Public);
        }
        std::future::ready(Ok(result))
    }

    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build()).with_instructions(
            "Tools for taking part in an AgentsAssemble room from this conversation. room_join takes the user's invite URL and makes this conversation itself a participant; room_read shows the room, room_say posts, room_wait_next waits for others. connection_id is private and is passed unchanged to later tools."
        )
    }
}
