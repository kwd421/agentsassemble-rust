//! External attendee transport owns its credential and retry identities, never the room store.
use agentsassemble_persistence::{
    ATTENDEE_INVITE_PREFIX, ATTENDEE_SESSION_PREFIX, AttendeeCleanupDelivery,
    AttendeeCleanupReport, AttendeeInterruptReport, AttendeeRandomRequest, AttendeeToolReadRequest,
};
use agentsassemble_protocol::CommandResolution;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::time::Duration;
use url::Url;
use uuid::Uuid;

use crate::room_client_transport::{self, JoinUrlError, ResponseReadError};

const RESPONSE_LIMIT: usize = 2 * agentsassemble_domain::MAX_ATTACHMENT_BYTES
    + 200 * (agentsassemble_domain::MAX_MESSAGE_CHARACTERS * 4 + 8192);

#[derive(Debug, thiserror::Error)]
#[error("{code}{suffix}", suffix = if .message.is_empty() { String::new() } else { format!(": {}", .message) })]
pub struct AttendeeClientError {
    pub code: String,
    pub message: String,
    pub resolution: Option<CommandResolution>,
}

impl AttendeeClientError {
    /// Whether another attempt can resolve transport or explicitly unresolved command custody.
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        self.resolution == Some(CommandResolution::Unresolved)
            || (self.resolution.is_none()
                && matches!(
                    self.code.as_str(),
                    "attendee_transport_unresolved"
                        | "invalid_attendee_response"
                        | "attendee_connect_timeout"
                        | "attendee_socket_unresolved"
                        | "attendee_socket_closed"
                        | "attendee_ack_unresolved"
                ))
    }

    pub(crate) fn local(code: &str) -> Self {
        Self {
            code: code.to_owned(),
            message: String::new(),
            resolution: None,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct AttendeeJoined {
    pub room_id: String,
    pub room_uid: Uuid,
    pub participant_id: String,
    pub provider_kind: String,
    pub expires_at: DateTime<Utc>,
    /// Whether the host speaks the chunked attachment-transfer endpoints.
    /// Older hosts omit this and keep the previous inline-only behaviour.
    #[serde(default)]
    pub attachment_transfers: bool,
}

#[derive(Deserialize)]
struct Admission {
    session_bearer: String,
    #[serde(flatten)]
    joined: AttendeeJoined,
}

#[derive(Serialize)]
struct JoinRequest {
    request_id: Uuid,
    client_secret: String,
    provider: String,
    display_name: String,
}

pub struct RoomAttendeeClient {
    http: Client,
    pub(crate) server: Url,
    invite_bearer: String,
    pending: JoinRequest,
    admission: Option<Admission>,
    expected_room: Option<(String, Uuid)>,
    invitation_identity: [u8; 32],
}

impl RoomAttendeeClient {
    /// Prepares a provider-bound invite without starting a runtime or sending a request.
    ///
    /// # Errors
    /// Rejects malformed/wrong-purpose invitations, unsupported providers and HTTP setup failure.
    pub fn new(
        invite_url: &str,
        provider: &str,
        display_name: &str,
    ) -> Result<Self, AttendeeClientError> {
        let (server, invite_bearer) = room_client_transport::parse_join_invite(invite_url)
            .map_err(|error| {
                AttendeeClientError::local(match error {
                    JoinUrlError::Invalid => "invalid_attendee_invite",
                    JoinUrlError::CredentialRequired => "attendee_invite_required",
                    JoinUrlError::InvalidServer => "invalid_room_server_url",
                })
            })?;
        if !invite_bearer.starts_with(ATTENDEE_INVITE_PREFIX) {
            return Err(AttendeeClientError::local("attendee_invite_required"));
        }
        let provider = agentsassemble_provider::registered_provider_kind(provider)
            .ok_or_else(|| AttendeeClientError::local("unsupported_provider"))?;
        let http = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .timeout(agentsassemble_domain::runtime_shutdown::ATTENDEE_HTTP_TIMEOUT)
            .build()
            .map_err(|_| AttendeeClientError::local("http_client_unavailable"))?;
        Ok(Self {
            http,
            invitation_identity: Sha256::digest(format!("{server}\n{invite_bearer}").as_bytes())
                .into(),
            server,
            invite_bearer,
            pending: JoinRequest {
                request_id: Uuid::new_v4(),
                client_secret: URL_SAFE_NO_PAD.encode(rand::random::<[u8; 32]>()),
                provider: provider.to_owned(),
                display_name: display_name.to_owned(),
            },
            admission: None,
            expected_room: None,
        })
    }

    /// Binds a local creation handoff to the browser's exact room incarnation.
    ///
    /// # Errors
    /// Rejects invalid destinations before creating the retained admission identity.
    pub fn for_room(
        invite_url: &str,
        provider: &str,
        display_name: &str,
        room_id: String,
        incarnation: Uuid,
    ) -> Result<Self, AttendeeClientError> {
        if room_id.is_empty() || incarnation.is_nil() {
            return Err(AttendeeClientError::local("attendee_room_mismatch"));
        }
        let mut client = Self::new(invite_url, provider, display_name)?;
        client.expected_room = Some((room_id, incarnation));
        Ok(client)
    }

    /// Identifies an invitation for local task deduplication; it conveys no admission authority.
    #[must_use]
    pub const fn invitation_identity(&self) -> [u8; 32] {
        self.invitation_identity
    }

    /// Recovers admission with the original client secret and request identity after response loss.
    ///
    /// # Errors
    /// Returns redacted transport, admission and response-validation failures.
    pub async fn join(&mut self) -> Result<AttendeeJoined, AttendeeClientError> {
        if let Some(admission) = &self.admission {
            return Ok(admission.joined.clone());
        }
        let value = read_response(
            self.http
                .post(self.endpoint("join")?)
                .bearer_auth(&self.invite_bearer)
                .json(&self.pending),
        )
        .await?;
        let admission: Admission = decode(value)?;
        if !admission
            .session_bearer
            .starts_with(ATTENDEE_SESSION_PREFIX)
            || admission.joined.provider_kind != self.pending.provider
            || admission.joined.room_uid.is_nil()
            || admission.joined.room_id.is_empty()
            || admission.joined.participant_id.is_empty()
            || self.expected_room.as_ref().is_some_and(|(id, uid)| {
                admission.joined.room_id != *id || admission.joined.room_uid != *uid
            })
        {
            return Err(AttendeeClientError::local("invalid_attendee_admission"));
        }
        let joined = admission.joined.clone();
        self.admission = Some(admission);
        self.invite_bearer.clear();
        self.pending.client_secret.clear();
        Ok(joined)
    }

    /// Whether the host advertised chunked attachment transfers at join.
    /// Without the advertisement the client keeps the previous inline-only behaviour.
    #[must_use]
    pub fn attachment_transfers(&self) -> bool {
        self.admission
            .as_ref()
            .is_some_and(|admission| admission.joined.attachment_transfers)
    }

    /// Begins one client-streamed upload under the current turn; the host checks
    /// the current room limit and quota before any file byte is sent.
    ///
    /// # Errors
    /// Returns missing admission, transport, custody or response failures.
    pub async fn upload_begin(
        &self,
        connection: Uuid,
        request: &Value,
    ) -> Result<Value, AttendeeClientError> {
        self.post("tool/upload/begin", Some(connection), request)
            .await
    }

    /// Streams one file body at the accepted offset without holding it in memory.
    ///
    /// # Errors
    /// Returns transport, custody, offset-conflict or interruption failures.
    pub async fn upload_data(
        &self,
        connection: Uuid,
        handle: &str,
        offset: usize,
        turn_generation: u64,
        execution_id: &str,
        body: reqwest::Body,
    ) -> Result<Value, AttendeeClientError> {
        let operation = format!("tool/upload/{handle}/data");
        let request = self
            .http
            .post(self.endpoint(&operation)?)
            .bearer_auth(self.bearer()?)
            .header("x-attendee-connection-id", connection.to_string())
            .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
            .query(&[
                ("offset", offset.to_string()),
                ("turn_generation", turn_generation.to_string()),
                ("execution_id", execution_id.to_owned()),
            ])
            .body(body);
        read_response(request).await
    }

    /// Finishes one client-streamed upload into a pending attachment receipt.
    ///
    /// # Errors
    /// Returns transport, custody or validation failures.
    pub async fn upload_finish(
        &self,
        connection: Uuid,
        handle: &str,
        turn_generation: u64,
        execution_id: &str,
    ) -> Result<Value, AttendeeClientError> {
        let operation = format!("tool/upload/{handle}/finish");
        let mut url = self.endpoint(&operation)?;
        url.query_pairs_mut()
            .append_pair("turn_generation", &turn_generation.to_string())
            .append_pair("execution_id", execution_id);
        let request = self
            .http
            .post(url)
            .bearer_auth(self.bearer()?)
            .header("x-attendee-connection-id", connection.to_string());
        read_response(request).await
    }

    /// Discards one incomplete client-streamed upload in its exact custody.
    ///
    /// # Errors
    /// Returns transport failures; custody failures are already terminal.
    pub async fn upload_cancel(
        &self,
        connection: Uuid,
        handle: &str,
        turn_generation: u64,
        execution_id: &str,
    ) -> Result<(), AttendeeClientError> {
        let operation = format!("tool/upload/{handle}");
        let mut url = self.endpoint(&operation)?;
        url.query_pairs_mut()
            .append_pair("turn_generation", &turn_generation.to_string())
            .append_pair("execution_id", execution_id);
        let request = self
            .http
            .delete(url)
            .bearer_auth(self.bearer()?)
            .header("x-attendee-connection-id", connection.to_string());
        let value = read_response(request).await?;
        committed(&value)
    }

    /// Streams one large published attachment without holding it in memory.
    ///
    /// # Errors
    /// Returns transport, custody, budget or response failures.
    pub async fn download_attachment(
        &self,
        connection: Uuid,
        attachment_id: &str,
        turn_generation: u64,
        execution_id: &str,
    ) -> Result<
        (
            agentsassemble_persistence::MessageAttachmentMetadata,
            impl futures_util::Stream<Item = Result<axum::body::Bytes, AttendeeClientError>> + Send,
        ),
        AttendeeClientError,
    > {
        use futures_util::StreamExt;
        let url = self.endpoint("tool/attachment/download")?;
        let mut response = self
            .http
            .get(url)
            .bearer_auth(self.bearer()?)
            .header("x-attendee-connection-id", connection.to_string())
            .query(&[
                ("attachment_id", attachment_id.to_owned()),
                ("turn_generation", turn_generation.to_string()),
                ("execution_id", execution_id.to_owned()),
            ])
            .send()
            .await
            .map_err(|_| AttendeeClientError::local("attendee_transport_unresolved"))?;
        if !response.status().is_success() {
            let mut body = Vec::new();
            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(|_| AttendeeClientError::local("attendee_transport_unresolved"))?
            {
                if body.len().saturating_add(chunk.len()) > 16 * 1024 {
                    return Err(AttendeeClientError::local("invalid_attendee_response"));
                }
                body.extend_from_slice(&chunk);
            }
            let value: Value = serde_json::from_slice(&body)
                .map_err(|_| AttendeeClientError::local("invalid_attendee_response"))?;
            let (code, resolution) =
                room_client_transport::remote_rejection(&value, "attendee_remote_rejection");
            return Err(AttendeeClientError {
                message: preserved_message(&value, &code),
                code,
                resolution,
            });
        }
        let headers = response.headers().clone();
        let metadata: agentsassemble_persistence::MessageAttachmentMetadata =
            header_value(&headers, "x-attachment-metadata")
                .and_then(|value| {
                    base64::Engine::decode(
                        &base64::engine::general_purpose::STANDARD,
                        value.as_bytes(),
                    )
                    .ok()
                })
                .and_then(|bytes| serde_json::from_slice(&bytes).ok())
                .filter(
                    |metadata: &agentsassemble_persistence::MessageAttachmentMetadata| {
                        metadata.id == attachment_id
                    },
                )
                .ok_or_else(|| AttendeeClientError::local("invalid_attendee_response"))?;
        let stream = response.bytes_stream().map(|next| {
            next.map_err(|_| AttendeeClientError::local("attendee_transport_unresolved"))
        });
        Ok((metadata, stream))
    }

    /// Reads only this attendee's pending exact runtime cleanup, even after ordinary expiry.
    ///
    /// # Errors
    /// Returns missing admission, transport, custody or response failures.
    pub async fn cleanup(&self) -> Result<Option<AttendeeCleanupDelivery>, AttendeeClientError> {
        let response = read_response(
            self.http
                .get(self.endpoint("cleanup")?)
                .bearer_auth(self.bearer()?),
        )
        .await?;
        let stop = response
            .get("stop")
            .cloned()
            .ok_or_else(|| AttendeeClientError::local("invalid_attendee_response"))?;
        decode(stop)
    }

    /// Requests self-leave with the caller-retained identity; shutdown completion is separate.
    ///
    /// # Errors
    /// Returns transport uncertainty or rejected exact membership custody.
    pub async fn leave(&self, request_id: Uuid) -> Result<(), AttendeeClientError> {
        committed(
            &self
                .post("leave", None, &json!({"request_id":request_id}))
                .await?,
        )
    }

    /// Reports positively observed absence for the exact delivered runtime.
    ///
    /// # Errors
    /// Returns transport uncertainty, changed report identity and custody failures.
    pub async fn report_cleanup(
        &self,
        report: &AttendeeCleanupReport,
    ) -> Result<(), AttendeeClientError> {
        committed(&self.post("cleanup", None, report).await?)
    }

    /// Reports an exact interruption through the current connection.
    ///
    /// # Errors
    /// Returns transport uncertainty and rejected interruption custody.
    pub async fn report_interrupt(
        &self,
        connection: Uuid,
        report: &AttendeeInterruptReport,
    ) -> Result<(), AttendeeClientError> {
        committed(&self.post("interrupt", Some(connection), report).await?)
    }

    /// Reads authorized lobby context or a bound input attachment for one exact turn.
    ///
    /// # Errors
    /// Returns stale connection/turn, transport and response failures.
    pub async fn read_tool(
        &self,
        connection: Uuid,
        request: &AttendeeToolReadRequest,
    ) -> Result<crate::AttendeeToolReadResponse, AttendeeClientError> {
        decode(self.post("tool/read", Some(connection), request).await?)
    }

    /// Obtains the server-owned random result; retries must retain this exact request.
    ///
    /// # Errors
    /// Returns rejected tool authority, request conflicts, transport and response failures.
    pub async fn random(
        &self,
        connection: Uuid,
        request: &AttendeeRandomRequest,
    ) -> Result<agentsassemble_domain::RoomRandomResult, AttendeeClientError> {
        let value = self.post("tool/random", Some(connection), request).await?;
        committed(&value)?;
        decode(value["result"].clone())
    }

    pub(crate) fn bearer(&self) -> Result<&str, AttendeeClientError> {
        self.admission
            .as_ref()
            .map(|admission| admission.session_bearer.as_str())
            .ok_or_else(|| AttendeeClientError::local("attendee_not_admitted"))
    }

    pub(crate) fn endpoint(&self, operation: &str) -> Result<Url, AttendeeClientError> {
        self.server
            .join(&format!("api/room-attendee/{operation}"))
            .map_err(|_| AttendeeClientError::local("invalid_room_server_url"))
    }

    pub(super) async fn post(
        &self,
        operation: &str,
        connection: Option<Uuid>,
        body: &impl Serialize,
    ) -> Result<Value, AttendeeClientError> {
        let mut request = self
            .http
            .post(self.endpoint(operation)?)
            .bearer_auth(self.bearer()?)
            .json(body);
        if let Some(connection) = connection {
            request = request.header("x-attendee-connection-id", connection.to_string());
        }
        read_response(request).await
    }
}

fn committed(value: &Value) -> Result<(), AttendeeClientError> {
    if value["resolution"] == "committed" {
        Ok(())
    } else {
        Err(AttendeeClientError::local("invalid_attendee_response"))
    }
}

pub(crate) fn decode<T: serde::de::DeserializeOwned>(
    value: Value,
) -> Result<T, AttendeeClientError> {
    serde_json::from_value(value)
        .map_err(|_| AttendeeClientError::local("invalid_attendee_response"))
}

async fn read_response(request: reqwest::RequestBuilder) -> Result<Value, AttendeeClientError> {
    let (status, value) = room_client_transport::read_json_response(request, RESPONSE_LIMIT)
        .await
        .map_err(|error| {
            AttendeeClientError::local(match error {
                ResponseReadError::Transport => "attendee_transport_unresolved",
                ResponseReadError::TooLarge => "attendee_response_too_large",
                ResponseReadError::InvalidJson => "invalid_attendee_response",
            })
        })?;
    if !status.is_success() {
        let (code, resolution) =
            room_client_transport::remote_rejection(&value, "attendee_remote_rejection");
        return Err(AttendeeClientError {
            message: preserved_message(&value, &code),
            code,
            resolution,
        });
    }
    Ok(value)
}

fn header_value(headers: &reqwest::header::HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}

fn preserved_message(value: &Value, code: &str) -> String {
    if code.starts_with("upload_")
        || matches!(
            code,
            "attachment_too_large"
                | "attachment_download_budget_exhausted"
                | "permission_denied"
                | "room_attachment_quota_reached"
                | "workspace_file_failed"
        )
    {
        value["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .to_owned()
    } else {
        String::new()
    }
}
