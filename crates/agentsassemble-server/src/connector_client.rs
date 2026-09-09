//! Transport custody for an external current conversation; it has no local room or provider authority.
use agentsassemble_protocol::{CommandResolution, RoomAction};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use reqwest::{Client, RequestBuilder};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[path = "connector_client_transport.rs"]
pub(crate) mod transport;
use sha2::{Digest, Sha256};
use std::{sync::Arc, time::Duration};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use transport::{parse_invite, read_response};
use url::Url;
use uuid::Uuid;

const COMMAND_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, thiserror::Error)]
#[error("{code}")]
pub struct ConnectorClientError {
    pub code: String,
    pub resolution: Option<CommandResolution>,
}
impl ConnectorClientError {
    fn local(code: &str) -> Self {
        Self {
            code: code.to_owned(),
            resolution: None,
        }
    }
}

#[derive(Clone, Serialize)]
pub struct ConnectorJoined {
    pub room_id: String,
    pub room_uid: Uuid,
    pub participant_id: String,
    pub display_name: String,
    pub last_seq: i64,
}

#[derive(Deserialize)]
struct Admission {
    session_bearer: String,
    room_id: String,
    room_uid: Uuid,
    participant_id: String,
}
struct PendingJoin {
    request_id: Uuid,
    client_secret: String,
    invite_bearer: String,
    attempted: bool,
}
enum JoinState {
    Pending(PendingJoin),
    Admitted(Admission),
    Ready(Arc<Session>),
}
struct Session {
    bearer: String,
    joined: ConnectorJoined,
    wait_cursor: Mutex<i64>,
}
struct PendingCommand {
    request_id: Uuid,
    action: RoomAction,
    payload: Value,
    hash: String,
}

pub struct RoomConnectorClient {
    http: Client,
    server: Url,
    invite_fingerprint: [u8; 32],
    pub(crate) display_name: String,
    state: Mutex<JoinState>,
    pending_command: Mutex<Option<PendingCommand>>,
    cancellation: CancellationToken,
}

impl RoomConnectorClient {
    /// Prepares one invite under an optional exact remote-service destination allowlist.
    ///
    /// # Errors
    /// Rejects malformed/wrong-purpose invites, unapproved destinations and HTTP client setup failures.
    pub fn new(
        invite_url: &str,
        display_name: &str,
        allowed_servers: Option<&[Url]>,
    ) -> Result<Self, ConnectorClientError> {
        let (server, invite_bearer) = parse_invite(invite_url)?;
        if allowed_servers.is_some_and(|allowed| !allowed.contains(&server)) {
            return Err(ConnectorClientError::local("room_server_not_allowed"));
        }
        let name = if display_name.trim().is_empty() {
            "External AI"
        } else {
            display_name
        };
        let fingerprint = Sha256::digest(format!("{server}\n{invite_bearer}").as_bytes()).into();
        let http = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .build()
            .map_err(|_| ConnectorClientError::local("http_client_unavailable"))?;
        Ok(Self {
            http,
            server,
            invite_fingerprint: fingerprint,
            display_name: name.to_owned(),
            state: Mutex::new(JoinState::Pending(PendingJoin {
                request_id: Uuid::new_v4(),
                client_secret: URL_SAFE_NO_PAD.encode(rand::random::<[u8; 32]>()),
                invite_bearer,
                attempted: false,
            })),
            pending_command: Mutex::new(None),
            cancellation: CancellationToken::new(),
        })
    }

    /// Identifies the exact invitation for a process-owned connection registry; not authorization.
    #[must_use]
    pub const fn invitation_identity(&self) -> [u8; 32] {
        self.invite_fingerprint
    }

    /// Recovers an uncertain admission with the same private client/request identity.
    ///
    /// # Errors
    /// Reports transport uncertainty, rejected admission or malformed public room responses.
    pub async fn join(&self) -> Result<ConnectorJoined, ConnectorClientError> {
        let mut state = self.state.lock().await;
        if self.cancellation.is_cancelled() {
            return Err(ConnectorClientError::local("connector_closed"));
        }
        if let JoinState::Ready(session) = &*state {
            let snapshot = self.get("read", &[], &session.bearer, false).await?;
            let mut joined = session.joined.clone();
            joined.last_seq = sequence(&snapshot)?;
            return Ok(joined);
        }
        if let JoinState::Pending(pending) = &mut *state {
            pending.attempted = true;
            let response = self.request(self.http.post(self.endpoint("join")).bearer_auth(&pending.invite_bearer).json(&json!({"request_id":pending.request_id,"client_secret":pending.client_secret,"display_name":self.display_name})).timeout(Duration::from_secs(10))).await?;
            let admission: Admission = serde_json::from_value(response)
                .map_err(|_| ConnectorClientError::local("invalid_connector_admission"))?;
            *state = JoinState::Admitted(admission);
        }
        if let JoinState::Admitted(admission) = &*state {
            let snapshot = self
                .get("read", &[], &admission.session_bearer, false)
                .await?;
            let last_seq = sequence(&snapshot)?;
            let joined = ConnectorJoined {
                room_id: admission.room_id.clone(),
                room_uid: admission.room_uid,
                participant_id: admission.participant_id.clone(),
                display_name: self.display_name.clone(),
                last_seq,
            };
            *state = JoinState::Ready(Arc::new(Session {
                bearer: admission.session_bearer.clone(),
                joined,
                wait_cursor: Mutex::new(last_seq),
            }));
        }
        match &*state {
            JoinState::Ready(session) => Ok(session.joined.clone()),
            _ => Err(ConnectorClientError::local("connector_not_ready")),
        }
    }

    /// Cancels preparation only before this owner has attempted any admission I/O.
    pub(crate) async fn cancel_prepared(&self) -> bool {
        let state = self.state.lock().await;
        if matches!(&*state, JoinState::Pending(pending) if !pending.attempted) {
            self.close();
            true
        } else {
            false
        }
    }

    /// Reads the current public view without consuming pending-message observation.
    ///
    /// # Errors
    /// Reports ended sessions, bounded-response failures or network uncertainty.
    pub async fn read(&self) -> Result<Value, ConnectorClientError> {
        let session = self.session().await?;
        self.get("read", &[], &session.bearer, false).await
    }

    /// Searches the exact joined room through its canonical public search owner.
    ///
    /// # Errors
    /// Reports revoked authority, invalid search scope/input or network failures.
    pub async fn search(
        &self,
        channel: &str,
        query: &str,
        cursor: &str,
    ) -> Result<Value, ConnectorClientError> {
        let session = self.session().await?;
        self.get(
            "search",
            &[
                ("channel_id", channel.to_owned()),
                ("q", query.to_owned()),
                ("cursor", cursor.to_owned()),
            ],
            &session.bearer,
            false,
        )
        .await
    }

    /// Follows a search result's exact channel and event identifiers.
    ///
    /// # Errors
    /// Reports revoked authority, missing context or network failures.
    pub async fn context(&self, channel: &str, event: &str) -> Result<Value, ConnectorClientError> {
        let session = self.session().await?;
        self.get(
            "context",
            &[
                ("channel_id", channel.to_owned()),
                ("event_id", event.to_owned()),
            ],
            &session.bearer,
            false,
        )
        .await
    }

    /// Reads a canonical vote summary without creating an event.
    ///
    /// # Errors
    /// Reports revoked authority, unknown votes or network failures.
    pub async fn vote_summary(&self, vote_id: &str) -> Result<Value, ConnectorClientError> {
        let session = self.session().await?;
        self.get(
            "vote",
            &[("vote_id", vote_id.to_owned())],
            &session.bearer,
            false,
        )
        .await
    }

    /// Waits without a model-visible deadline; cancellation leaves the cursor unconsumed.
    ///
    /// # Errors
    /// Reports disconnect, expiry, required resynchronization or transport failure.
    pub async fn wait_next(&self) -> Result<Value, ConnectorClientError> {
        let session = self.session().await?;
        let mut cursor = session.wait_cursor.lock().await;
        let response = self
            .get(
                "wait",
                &[("after_seq", cursor.to_string())],
                &session.bearer,
                true,
            )
            .await?;
        *cursor = sequence(&response)?;
        Ok(response)
    }

    /// Keeps an uncertain mutation's request identity until its owner resolves the result.
    ///
    /// # Errors
    /// Rejects a different command while a receipt is unresolved and exposes remote/transport failures.
    pub async fn command(
        &self,
        action: RoomAction,
        payload: Value,
    ) -> Result<Value, ConnectorClientError> {
        let session = self.session().await?;
        let hash = agentsassemble_domain::canonical_payload_hash(&payload);
        let mut pending = self.pending_command.lock().await;
        if let Some(current) = &*pending {
            if current.action != action || current.hash != hash {
                return Err(ConnectorClientError::local(
                    "previous_connector_command_unresolved",
                ));
            }
        } else {
            *pending = Some(PendingCommand {
                request_id: Uuid::new_v4(),
                action,
                payload,
                hash,
            });
        }
        let current = pending
            .as_ref()
            .ok_or_else(|| ConnectorClientError::local("connector_command_missing"))?;
        let result = self.request(self.http.post(self.endpoint("command")).bearer_auth(&session.bearer).json(&json!({"request_id":current.request_id.to_string(),"action":current.action,"payload":current.payload})).timeout(COMMAND_TIMEOUT)).await;
        match &result {
            Ok(response)
                if response.get("resolution").and_then(Value::as_str) == Some("committed") =>
            {
                *pending = None;
            }
            Ok(_) => {
                return Err(ConnectorClientError::local(
                    "invalid_connector_command_response",
                ));
            }
            Err(error) if error.resolution == Some(CommandResolution::Rejected) => {
                *pending = None;
            }
            Err(_) => {}
        }
        result
    }

    /// Closes this process's transport custody; it does not claim a room leave or provider stop.
    pub fn close(&self) {
        self.cancellation.cancel();
    }

    async fn session(&self) -> Result<Arc<Session>, ConnectorClientError> {
        if self.cancellation.is_cancelled() {
            return Err(ConnectorClientError::local("connector_closed"));
        }
        match &*self.state.lock().await {
            JoinState::Ready(session) => Ok(session.clone()),
            _ => Err(ConnectorClientError::local("connector_not_joined")),
        }
    }
    fn endpoint(&self, operation: &str) -> Url {
        let mut endpoint = self.server.clone();
        endpoint.set_path(&format!(
            "{}api/room-connector/{operation}",
            self.server.path()
        ));
        endpoint
    }
    async fn get(
        &self,
        operation: &str,
        query: &[(&str, String)],
        bearer: &str,
        wait: bool,
    ) -> Result<Value, ConnectorClientError> {
        let request = self
            .http
            .get(self.endpoint(operation))
            .bearer_auth(bearer)
            .query(query);
        self.request(if wait {
            request
        } else {
            request.timeout(COMMAND_TIMEOUT)
        })
        .await
    }
    async fn request(&self, request: RequestBuilder) -> Result<Value, ConnectorClientError> {
        tokio::select! {
            () = self.cancellation.cancelled() => Err(ConnectorClientError::local("connector_closed")),
            response = read_response(request) => response,
        }
    }
}

fn sequence(value: &Value) -> Result<i64, ConnectorClientError> {
    value
        .get("last_seq")
        .and_then(Value::as_i64)
        .filter(|seq| *seq >= 0)
        .ok_or_else(|| ConnectorClientError::local("invalid_connector_cursor"))
}
