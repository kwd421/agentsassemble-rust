use agentsassemble_protocol::CommandResolution;
use reqwest::{RequestBuilder, StatusCode};
use serde_json::Value;
use url::Url;

#[derive(Clone, Copy)]
pub(crate) enum JoinUrlError {
    Invalid,
    CredentialRequired,
    InvalidServer,
}

#[derive(Clone, Copy)]
pub(crate) enum ResponseReadError {
    Transport,
    TooLarge,
    InvalidJson,
}

pub(crate) fn normalize_server(value: &str) -> Result<Url, JoinUrlError> {
    let mut url = Url::parse(value).map_err(|_| JoinUrlError::InvalidServer)?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(JoinUrlError::InvalidServer);
    }
    url.set_path(&format!("{}/", url.path().trim_end_matches('/')));
    Ok(url)
}

pub(crate) fn parse_join_invite(value: &str) -> Result<(Url, String), JoinUrlError> {
    let mut invite = Url::parse(value.trim()).map_err(|_| JoinUrlError::Invalid)?;
    let tokens: Vec<_> = invite
        .query_pairs()
        .filter(|(key, _)| key == "token")
        .map(|(_, value)| value.into_owned())
        .collect();
    let [token] = tokens.as_slice() else {
        return Err(JoinUrlError::CredentialRequired);
    };
    let path = invite
        .path()
        .trim_end_matches('/')
        .strip_suffix("/join")
        .ok_or(JoinUrlError::Invalid)?
        .to_owned();
    invite.set_path(&path);
    invite.set_query(None);
    invite.set_fragment(None);
    Ok((normalize_server(invite.as_str())?, token.clone()))
}

pub(crate) async fn read_json_response(
    request: RequestBuilder,
    limit: usize,
) -> Result<(StatusCode, Value), ResponseReadError> {
    let mut response = request
        .send()
        .await
        .map_err(|_| ResponseReadError::Transport)?;
    let status = response.status();
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| ResponseReadError::Transport)?
    {
        if body.len().saturating_add(chunk.len()) > limit {
            return Err(ResponseReadError::TooLarge);
        }
        body.extend_from_slice(&chunk);
    }
    let value = serde_json::from_slice(&body).map_err(|_| ResponseReadError::InvalidJson)?;
    Ok((status, value))
}

pub(crate) fn remote_rejection(
    value: &Value,
    default_code: &str,
) -> (String, Option<CommandResolution>) {
    let code = value
        .pointer("/error/code")
        .and_then(Value::as_str)
        .filter(|value| {
            value.len() <= 64
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        })
        .unwrap_or(default_code)
        .to_owned();
    let resolution = value
        .get("resolution")
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok());
    (code, resolution)
}
