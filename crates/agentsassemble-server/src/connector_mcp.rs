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
    Choose, Connection, Context, Join, Roll, Say, Search, VoteCast, VoteCreate, VoteTarget,
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
        action: RoomAction,
        payload: Value,
    ) -> Result<String, String> {
        encode(
            &self
                .hub
                .client(id)?
                .command(action, payload)
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
        description = "Join the current AI conversation using the complete unchanged AgentsAssemble /join?token= URL. Do not fetch the URL or launch a provider."
    )]
    async fn room_join(&self, Parameters(input): Parameters<Join>) -> Result<String, String> {
        encode(
            &self
                .hub
                .join(&input.invite_url, &input.display_name)
                .await?,
        )
    }

    #[tool(description = "Read the bounded current room context and finalized public messages.")]
    async fn room_read(&self, Parameters(input): Parameters<Connection>) -> Result<String, String> {
        encode(
            &self
                .hub
                .client(&input.connection_id)?
                .read()
                .await
                .map_err(|error| error.code)?,
        )
    }

    #[tool(
        description = "Search readable room history. Pass next_cursor unchanged for another page."
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

    #[tool(description = "Read bounded surrounding messages for an exact room search result.")]
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

    #[tool(description = "Send a substantive public room contribution as this participant.")]
    async fn room_say(&self, Parameters(input): Parameters<Say>) -> Result<String, String> {
        self.command(
            &input.connection_id,
            RoomAction::MessageSend,
            json!({"content": input.content}),
        )
        .await
    }

    #[tool(description = "Create a bounded single-choice room poll.")]
    async fn room_vote_create(
        &self,
        Parameters(input): Parameters<VoteCreate>,
    ) -> Result<String, String> {
        self.command(&input.connection_id, RoomAction::MessageSend, json!({"kind":"vote", "vote_question":input.question, "vote_options":input.options, "vote_duration_seconds":input.duration_seconds})).await
    }

    #[tool(description = "Cast or replace this participant's ballot.")]
    async fn room_vote_cast(
        &self,
        Parameters(input): Parameters<VoteCast>,
    ) -> Result<String, String> {
        self.command(
            &input.connection_id,
            RoomAction::MessageSend,
            json!({"kind":"vote_cast", "vote_id":input.vote_id, "vote_choice":input.choice}),
        )
        .await
    }

    #[tool(description = "Withdraw this participant's ballot.")]
    async fn room_vote_withdraw(
        &self,
        Parameters(input): Parameters<VoteTarget>,
    ) -> Result<String, String> {
        self.command(
            &input.connection_id,
            RoomAction::MessageSend,
            json!({"kind":"vote_withdraw", "vote_id":input.vote_id}),
        )
        .await
    }

    #[tool(description = "Close a poll created by this participant.")]
    async fn room_vote_close(
        &self,
        Parameters(input): Parameters<VoteTarget>,
    ) -> Result<String, String> {
        self.command(
            &input.connection_id,
            RoomAction::MessageSend,
            json!({"kind":"vote_close", "vote_id":input.vote_id}),
        )
        .await
    }

    #[tool(description = "Read the canonical summary of a room poll.")]
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

    #[tool(description = "Roll bounded server-owned dice when tabletop tools are enabled.")]
    async fn room_roll_dice(&self, Parameters(input): Parameters<Roll>) -> Result<String, String> {
        self.command(
            &input.connection_id,
            RoomAction::RoomRandomRoll,
            json!({"notation":input.notation,"reason":input.reason}),
        )
        .await
    }

    #[tool(description = "Choose one bounded option using server-owned room randomness.")]
    async fn room_choose_random(
        &self,
        Parameters(input): Parameters<Choose>,
    ) -> Result<String, String> {
        self.command(
            &input.connection_id,
            RoomAction::RoomRandomChoose,
            json!({"options":input.options,"reason":input.reason}),
        )
        .await
    }

    #[tool(
        description = "Wait for another participant's public message without a model deadline. Other tools may run concurrently."
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
        description = "Leave this room and close its connection after the server confirms leave."
    )]
    async fn room_leave(
        &self,
        Parameters(input): Parameters<Connection>,
    ) -> Result<String, String> {
        encode(&self.hub.leave(&input.connection_id).await?)
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
            "Use room_join with the user's complete invite URL, then room_read. This connects the current conversation; do not launch another model or delegate. Pass connection_id unchanged to later tools and keep it private. Contribute with room_say, then room_wait_next."
        )
    }
}
