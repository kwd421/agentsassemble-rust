use agentsassemble_protocol::{
    CentralLoginAction, CentralLoginResult, LocalControlRequest, LocalControlResponse,
};
use serde::Serialize;
use tauri::AppHandle;
use url::Url;
use uuid::Uuid;

use super::{
    LocalRuntime,
    control::{TicketFailure, request_control},
    ensure_runtime, handle_ticket_result,
};

#[derive(Serialize)]
pub(crate) struct CentralLoginGrant {
    pub result: CentralLoginResult,
    pub redirect_uri: String,
}

impl LocalRuntime {
    pub(crate) fn central_login(
        &self,
        app: &AppHandle,
        action: CentralLoginAction,
        state: &str,
    ) -> Result<CentralLoginGrant, String> {
        let mut process = self
            .process
            .lock()
            .map_err(|_| "local runtime state lock is poisoned".to_owned())?;
        let runtime = ensure_runtime(&mut process, app)?;
        let redirect_uri = runtime
            .address
            .join("api/central-login/callback")
            .map_err(|_| "invalid runtime address".to_owned())?
            .to_string();
        let request_id = Uuid::new_v4().to_string();
        let response = request_control(
            runtime,
            &LocalControlRequest::CentralLogin {
                request_id: request_id.clone(),
                action,
                state: state.to_owned(),
            },
        );
        let result = response.and_then(|response| match response {
            LocalControlResponse::CentralLoginOk {
                request_id: response_id,
                result,
            } if response_id == request_id => Ok(CentralLoginGrant {
                result,
                redirect_uri,
            }),
            LocalControlResponse::Error {
                request_id: response_id,
                message,
                ..
            } if response_id == request_id => Err(TicketFailure::Rejected(message)),
            _ => Err(TicketFailure::Broken(
                "local runtime login response did not match the request".into(),
            )),
        });
        handle_ticket_result(&mut process, result)
    }

    pub(crate) fn open_central_google_login(
        &self,
        app: &AppHandle,
        raw: &str,
    ) -> Result<(), String> {
        let url = Url::parse(raw).map_err(|_| "invalid Google login URL".to_owned())?;
        let states: Vec<_> = url
            .query_pairs()
            .filter(|(key, _)| key == "state")
            .map(|(_, value)| value.into_owned())
            .collect();
        let [state] = states.as_slice() else {
            return Err("invalid Google login state".into());
        };
        let grant = self.central_login(app, CentralLoginAction::Poll, state)?;
        if !matches!(grant.result, CentralLoginResult::Pending { .. }) {
            return Err("Google login is no longer pending".into());
        }
        validate_authorization_url(&url, &grant.redirect_uri)?;
        open::that(url.as_str())
            .map_err(|_| "cannot open Google login in the system browser".into())
    }
}

fn validate_authorization_url(url: &Url, redirect_uri: &str) -> Result<(), String> {
    let pairs: Vec<_> = url.query_pairs().collect();
    let value = |name: &str| {
        let mut matches = pairs
            .iter()
            .filter(|(key, _)| key == name)
            .map(|(_, value)| value.as_ref());
        let first = matches.next();
        if matches.next().is_some() {
            None
        } else {
            first
        }
    };
    let challenge = value("code_challenge").unwrap_or_default();
    if url.scheme() != "https"
        || url.host_str() != Some("accounts.google.com")
        || url.port_or_known_default() != Some(443)
        || url.path() != "/o/oauth2/v2/auth"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || value("redirect_uri") != Some(redirect_uri)
        || value("response_type") != Some("code")
        || value("scope") != Some("openid")
        || value("code_challenge_method") != Some("S256")
        || challenge.len() != 43
        || !challenge
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
        || value("client_id").is_none_or(str::is_empty)
        || value("nonce").is_none_or(str::is_empty)
    {
        return Err("invalid Google authorization request".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_open_is_bound_to_google_pkce_and_owned_runtime() {
        let redirect = "http://127.0.0.1:45678/api/central-login/callback";
        let mut url = Url::parse("https://accounts.google.com/o/oauth2/v2/auth")
            .unwrap_or_else(|error| panic!("fixture: {error}"));
        url.query_pairs_mut().extend_pairs([
            ("redirect_uri", redirect),
            ("client_id", "fixture-client"),
            ("nonce", "fixture-nonce"),
            ("response_type", "code"),
            ("scope", "openid"),
            ("code_challenge_method", "S256"),
            ("code_challenge", &"a".repeat(43)),
        ]);
        assert!(validate_authorization_url(&url, redirect).is_ok());
        assert!(
            validate_authorization_url(&url, "http://127.0.0.1:45679/api/central-login/callback")
                .is_err()
        );
        let mut evil = url.clone();
        evil.set_host(Some("attacker.example"))
            .unwrap_or_else(|error| panic!("fixture: {error}"));
        assert!(validate_authorization_url(&evil, redirect).is_err());
        url.query_pairs_mut()
            .append_pair("redirect_uri", "https://attacker.example");
        assert!(validate_authorization_url(&url, redirect).is_err());
    }
}
