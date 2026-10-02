use agentsassemble_protocol::{
    CentralLoginAction, CentralLoginResult, LocalControlRequest, LocalControlResponse,
};
use serde::Serialize;
use std::{env, process::Stdio};
use tauri::AppHandle;
use url::Url;
use uuid::Uuid;

use super::{
    LocalRuntime, RuntimeProcess, abort_startup, capture_runtime_output,
    control::{TicketFailure, request_control},
    sidecar_executable, terminate_owned_runtime, validate_startup_record, wait_for_startup,
};

#[derive(Serialize)]
pub(crate) struct CentralLoginGrant {
    pub result: CentralLoginResult,
    pub redirect_uri: String,
}

impl LocalRuntime {
    pub(crate) fn central_login(
        &self,
        _app: &AppHandle,
        action: CentralLoginAction,
        state: &str,
    ) -> Result<CentralLoginGrant, String> {
        let mut process = self
            .login_process
            .lock()
            .map_err(|_| "local runtime state lock is poisoned".to_owned())?;
        if action == CentralLoginAction::Start {
            if let Some(runtime) = process.as_mut() {
                if runtime
                    .child
                    .try_wait()
                    .map_err(|_| "cannot inspect login process")?
                    .is_none()
                {
                    return Err("Google login is already running".into());
                }
                if let Some(mut stopped) = process.take() {
                    terminate_owned_runtime(&mut stopped);
                }
            }
            *process = Some(start_login_process()?);
        }
        let runtime = process.as_mut().ok_or("Google login is not running")?;
        let redirect_uri = runtime
            .address
            .join("api/central-login/callback")
            .map_err(|_| "invalid runtime address".to_owned())?
            .to_string();
        let request_id = Uuid::new_v4().to_string();
        let result = request_control(
            runtime,
            &LocalControlRequest::CentralLogin {
                request_id: request_id.clone(),
                action,
                state: state.to_owned(),
            },
            move |response| match response {
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
            },
        );
        let retire = action == CentralLoginAction::Cancel || result.is_err();
        if retire && let Some(mut stopped) = process.take() {
            terminate_owned_runtime(&mut stopped);
        }
        result.map_err(|error| match error {
            TicketFailure::Rejected(message)
            | TicketFailure::Unavailable(message)
            | TicketFailure::Broken(message) => message,
        })
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

// This child uses the normal signed supervisor but never receives a database,
// frontend, host registration or provider configuration. Control output is not logged.
fn start_login_process() -> Result<RuntimeProcess, String> {
    let desktop = env::current_exe().map_err(|error| error.to_string())?;
    let executable = sidecar_executable(&desktop)?;
    let mut command = crate::runtime_supervisor::command(&executable)?;
    let mut child = command
        .command_mut()
        .arg("--central-login-only")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("cannot start Google login listener: {error}"))?;
    let control = child.stdin.take();
    let output = child
        .stdout
        .take()
        .map(|stdout| capture_runtime_output(stdout, None));
    let result = match output {
        Some(output) if control.is_some() => wait_for_startup(&mut child, &output)
            .and_then(|record| validate_startup_record(&record))
            .map(|address| (output, address)),
        _ => Err("cannot open Google login control pipe".into()),
    };
    match result {
        Ok((output, address)) => Ok(RuntimeProcess {
            child,
            control,
            output,
            pending_response: None,
            address,
        }),
        Err(error) => {
            abort_startup(&mut child, control);
            Err(error)
        }
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
        || value("scope") != Some("openid profile")
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
            ("scope", "openid profile"),
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
