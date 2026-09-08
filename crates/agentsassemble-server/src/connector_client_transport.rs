use super::ConnectorClientError;
use crate::room_client_transport::{self, JoinUrlError, ResponseReadError};
use reqwest::RequestBuilder;
use serde_json::Value;
use url::Url;

// A finite canonical 200-event catch-up, including four-byte Unicode and event metadata.
const RESPONSE_LIMIT: usize = 200 * (agentsassemble_domain::MAX_MESSAGE_CHARACTERS * 4 + 8192);

pub(crate) fn normalize_server(value: &str) -> Result<Url, ConnectorClientError> {
    room_client_transport::normalize_server(value).map_err(invite_error)
}

pub(super) fn parse_invite(value: &str) -> Result<(Url, String), ConnectorClientError> {
    room_client_transport::parse_join_invite(value).map_err(invite_error)
}

fn invite_error(error: JoinUrlError) -> ConnectorClientError {
    ConnectorClientError::local(match error {
        JoinUrlError::Invalid => "invalid_connector_invite",
        JoinUrlError::CredentialRequired => "connector_invite_required",
        JoinUrlError::InvalidServer => "invalid_room_server_url",
    })
}

pub(super) async fn read_response(request: RequestBuilder) -> Result<Value, ConnectorClientError> {
    let (status, value) = room_client_transport::read_json_response(request, RESPONSE_LIMIT)
        .await
        .map_err(|error| {
            ConnectorClientError::local(match error {
                ResponseReadError::Transport => "connector_transport_unresolved",
                ResponseReadError::TooLarge => "connector_response_too_large",
                ResponseReadError::InvalidJson => "invalid_connector_response",
            })
        })?;
    if !status.is_success() {
        let (code, resolution) =
            room_client_transport::remote_rejection(&value, "connector_remote_rejection");
        return Err(ConnectorClientError { code, resolution });
    }
    Ok(value)
}
