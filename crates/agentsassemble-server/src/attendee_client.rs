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
#[error("{code}")]
pub struct AttendeeClientError {
    pub code: String,
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
            .timeout(Duration::from_secs(30))
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

    async fn post(
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
        return Err(AttendeeClientError { code, resolution });
    }
    Ok(value)
}
