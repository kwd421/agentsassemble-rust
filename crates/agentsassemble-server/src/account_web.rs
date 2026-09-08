use agentsassemble_domain::LOCAL_OPERATOR_USER_ID;
use agentsassemble_persistence::{
    AccountAuthority, AccountIdentity, GoogleAccount, PersistenceError,
};
use axum::{
    Json, Router,
    extract::{Request, State},
    http::{Method, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use serde_json::{Value, json};
use tower_http::set_header::SetResponseHeaderLayer;

use crate::{
    AppState, ConsumedProfileTicket,
    google_accounts::GoogleAccountError,
    http_api::{
        BodyDecodeError, DEVICE_CREDENTIAL_HEADER, PRIVATE_NO_STORE, bearer_credential,
        decode_json_body, ensure_empty_body, exact_tauri_cors,
    },
    human_browser_credential::fingerprint_browser_credential,
    human_session_http_authority::{
        HumanSessionBearerError, HumanSessionBearerResolution, resolve_human_session_bearer,
    },
    ingress_trust::{LocalIngress, PeerAddr, single_header},
};

const MAX_ACCOUNT_BODY_BYTES: usize = 32 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ChallengeRequest {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConnectRequest {
    credential: String,
    nonce: String,
    #[serde(default)]
    discard_guest_on_account_switch: bool,
}

pub(crate) fn routes() -> Router<AppState> {
    account_routes()
        .layer(SetResponseHeaderLayer::overriding(
            header::CACHE_CONTROL,
            PRIVATE_NO_STORE.clone(),
        ))
        .layer(
            exact_tauri_cors([Method::GET, Method::POST, Method::DELETE]).allow_headers([
                header::AUTHORIZATION,
                header::CONTENT_TYPE,
                DEVICE_CREDENTIAL_HEADER,
            ]),
        )
}

registered_routes! {
    fn account_routes<AppState>() {
        same_origin_public "/api/account" => get(status),
        same_origin_public "/api/account/google/challenge" => post(challenge),
        same_origin_public "/api/account/google" => post(connect).delete(disconnect),
    }
}

async fn status(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<Value>, AccountHttpError> {
    let identity = resolve_identity(&state, request.headers(), local_transport(&request)).await?;
    ensure_empty_body(request, MAX_ACCOUNT_BODY_BYTES).await?;
    let account = match identity {
        Some(identity) => state.store.google_account(&identity).await?,
        None => None,
    };
    Ok(Json(
        json!({"account": account.as_ref().map(account_view), "google": state.google_accounts.configuration()}),
    ))
}

async fn challenge(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<Value>, AccountHttpError> {
    let identity = resolve_identity(&state, request.headers(), local_transport(&request))
        .await?
        .ok_or_else(AccountHttpError::unauthorized)?;
    let _: ChallengeRequest = decode_json_body(request, MAX_ACCOUNT_BODY_BYTES).await?;
    Ok(Json(json!(state.google_accounts.start(identity).await?)))
}

async fn connect(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<Value>, AccountHttpError> {
    let identity = resolve_identity(&state, request.headers(), local_transport(&request))
        .await?
        .ok_or_else(AccountHttpError::unauthorized)?;
    let input: ConnectRequest = decode_json_body(request, MAX_ACCOUNT_BODY_BYTES).await?;
    let fingerprint = state
        .google_accounts
        .verify(&identity, &input.credential, &input.nonce)
        .await?;
    let commit = state
        .store
        .connect_google_account(
            &identity,
            &fingerprint,
            input.discard_guest_on_account_switch,
        )
        .await?;
    state.rooms.notify_account_commit(&commit).await;
    Ok(Json(
        json!({"status": "connected", "identity_switched": commit.identity_switched,
        "account": account_view(&commit.account), "user": {"user_id": commit.user.user_id,
        "participant_id": commit.user.participant_id, "display_name": commit.user.profile.display_name,
        "avatar_image_url": commit.user.profile.avatar_image_url}}),
    ))
}

async fn disconnect(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<Value>, AccountHttpError> {
    let identity = resolve_identity(&state, request.headers(), local_transport(&request))
        .await?
        .ok_or_else(AccountHttpError::unauthorized)?;
    ensure_empty_body(request, MAX_ACCOUNT_BODY_BYTES).await?;
    state.store.disconnect_google_account(&identity).await?;
    Ok(Json(json!({"status": "disconnected"})))
}

fn account_view(account: &GoogleAccount) -> Value {
    json!({"account_id": account.account_id, "provider": "google", "display_name": "",
        "email": "", "avatar_image_url": ""})
}

fn local_transport(request: &Request) -> bool {
    request
        .extensions()
        .get::<LocalIngress>()
        .copied()
        .zip(request.extensions().get::<PeerAddr>().copied())
        .is_some_and(|(ingress, peer)| ingress.authorizes(peer, request.headers()))
}

async fn resolve_identity(
    state: &AppState,
    headers: &axum::http::HeaderMap,
    local: bool,
) -> Result<Option<AccountIdentity>, AccountHttpError> {
    let device = if headers.contains_key(&DEVICE_CREDENTIAL_HEADER) {
        Some(
            single_header(headers, DEVICE_CREDENTIAL_HEADER)
                .and_then(fingerprint_browser_credential)
                .ok_or_else(AccountHttpError::unauthorized)?,
        )
    } else {
        None
    };
    let authority = if headers.contains_key(header::AUTHORIZATION) {
        single_header(headers, header::AUTHORIZATION).ok_or_else(AccountHttpError::unauthorized)?;
        let credential = bearer_credential(headers).ok_or_else(AccountHttpError::unauthorized)?;
        match resolve_human_session_bearer(state, credential).await {
            Ok(HumanSessionBearerResolution::Authorized(authorization)) => {
                AccountAuthority::HumanSession {
                    authorization,
                    browser_fingerprint: device.ok_or_else(AccountHttpError::unauthorized)?,
                }
            }
            Ok(HumanSessionBearerResolution::Other) => {
                // Native profile tickets cannot become public account credentials, even through
                // the authenticated public proxy. The ingress owner defines exact local transport.
                if !local || device.is_some() {
                    return Err(AccountHttpError::unauthorized());
                }
                match state.tickets.consume_profile(credential).await {
                    Ok(ConsumedProfileTicket::ServerOperator { principal_id })
                        if principal_id == LOCAL_OPERATOR_USER_ID =>
                    {
                        AccountAuthority::LocalOperator
                    }
                    _ => return Err(AccountHttpError::unauthorized()),
                }
            }
            Err(HumanSessionBearerError::Invalid) => return Err(AccountHttpError::unauthorized()),
            Err(HumanSessionBearerError::Persistence(error)) => return Err(error.into()),
        }
    } else if let Some(device) = device {
        AccountAuthority::BrowserDevice(device)
    } else {
        return Ok(None);
    };
    Ok(Some(state.store.account_identity(authority).await?))
}

struct AccountHttpError {
    status: StatusCode,
    code: std::borrow::Cow<'static, str>,
    message: String,
}

impl AccountHttpError {
    fn new(
        status: StatusCode,
        code: impl Into<std::borrow::Cow<'static, str>>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            status,
            code: code.into(),
            message: message.into(),
        }
    }
    fn unauthorized() -> Self {
        Self::new(
            StatusCode::UNAUTHORIZED,
            "account_authority_required",
            "Valid account authority is required.",
        )
    }
}
impl From<BodyDecodeError> for AccountHttpError {
    fn from(error: BodyDecodeError) -> Self {
        match error {
            BodyDecodeError::RequestTimeout => Self::new(
                StatusCode::REQUEST_TIMEOUT,
                "request_timeout",
                "Request body timed out.",
            ),
            BodyDecodeError::PayloadTooLarge => Self::new(
                StatusCode::PAYLOAD_TOO_LARGE,
                "payload_too_large",
                "Account request exceeds its limit.",
            ),
            BodyDecodeError::InvalidJson | BodyDecodeError::NonEmpty => Self::new(
                StatusCode::BAD_REQUEST,
                "bad_request",
                "Account request body is invalid.",
            ),
        }
    }
}
impl From<GoogleAccountError> for AccountHttpError {
    fn from(error: GoogleAccountError) -> Self {
        let status = match error.code {
            "google_login_capacity_exceeded" => StatusCode::TOO_MANY_REQUESTS,
            "google_login_not_configured" | "google_verification_unavailable" => {
                StatusCode::SERVICE_UNAVAILABLE
            }
            _ => StatusCode::UNAUTHORIZED,
        };
        Self::new(status, error.code, error.message)
    }
}
impl From<PersistenceError> for AccountHttpError {
    fn from(error: PersistenceError) -> Self {
        match error {
            PersistenceError::CommandRejected { code, message } => {
                let status = match code.as_bytes() {
                    b"account_switch_confirmation_required"
                    | b"account_link_conflict"
                    | b"account_identity_changed" => StatusCode::CONFLICT,
                    b"account_operator_boundary"
                    | b"account_switch_operator_forbidden"
                    | b"account_switch_unavailable"
                    | b"account_device_mismatch" => StatusCode::FORBIDDEN,
                    b"invalid_state" => StatusCode::SERVICE_UNAVAILABLE,
                    _ => StatusCode::UNAUTHORIZED,
                };
                Self::new(status, code, message)
            }
            _ => Self::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "persistence_failed",
                "Account persistence is unavailable.",
            ),
        }
    }
}
impl IntoResponse for AccountHttpError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(json!({"error": self.message, "code": self.code})),
        )
            .into_response()
    }
}

#[cfg(test)]
#[path = "account_web_tests.rs"]
mod tests;
