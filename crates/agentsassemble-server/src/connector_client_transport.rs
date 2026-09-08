use super::ConnectorClientError;
use reqwest::RequestBuilder;
use serde_json::Value;
use url::Url;

// A finite canonical 200-event catch-up, including four-byte Unicode and event metadata.
const RESPONSE_LIMIT: usize = 200 * (agentsassemble_domain::MAX_MESSAGE_CHARACTERS * 4 + 8192);

pub(crate) fn normalize_server(value: &str) -> Result<Url, ConnectorClientError> {
    let mut url =
        Url::parse(value).map_err(|_| ConnectorClientError::local("invalid_room_server_url"))?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(ConnectorClientError::local("invalid_room_server_url"));
    }
    url.set_path(&format!("{}/", url.path().trim_end_matches('/')));
    Ok(url)
}

pub(super) fn parse_invite(value: &str) -> Result<(Url, String), ConnectorClientError> {
    let mut invite = Url::parse(value.trim())
        .map_err(|_| ConnectorClientError::local("invalid_connector_invite"))?;
    let tokens: Vec<_> = invite
        .query_pairs()
        .filter(|(key, _)| key == "token")
        .map(|(_, value)| value.into_owned())
        .collect();
    let [token] = tokens.as_slice() else {
        return Err(ConnectorClientError::local("connector_invite_required"));
    };
    let path = invite
        .path()
        .trim_end_matches('/')
        .strip_suffix("/join")
        .ok_or_else(|| ConnectorClientError::local("invalid_connector_invite"))?
        .to_owned();
    invite.set_path(&path);
    invite.set_query(None);
    invite.set_fragment(None);
    Ok((normalize_server(invite.as_str())?, token.clone()))
}

pub(super) async fn read_response(request: RequestBuilder) -> Result<Value, ConnectorClientError> {
    let mut response = request
        .send()
        .await
        .map_err(|_| ConnectorClientError::local("connector_transport_unresolved"))?;
    let status = response.status();
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| ConnectorClientError::local("connector_transport_unresolved"))?
    {
        if body.len().saturating_add(chunk.len()) > RESPONSE_LIMIT {
            return Err(ConnectorClientError::local("connector_response_too_large"));
        }
        body.extend_from_slice(&chunk);
    }
    let value: Value = serde_json::from_slice(&body)
        .map_err(|_| ConnectorClientError::local("invalid_connector_response"))?;
    if !status.is_success() {
        let code = value
            .pointer("/error/code")
            .and_then(Value::as_str)
            .filter(|value| {
                value.len() <= 64
                    && value
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            })
            .unwrap_or("connector_remote_rejection");
        let resolution = value
            .get("resolution")
            .cloned()
            .and_then(|value| serde_json::from_value(value).ok());
        return Err(ConnectorClientError {
            code: code.to_owned(),
            resolution,
        });
    }
    Ok(value)
}
