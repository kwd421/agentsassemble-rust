use std::{
    collections::HashMap,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use agentsassemble_protocol::{CentralLoginAction, CentralLoginResult, LocalControlResponse};
use axum::{
    extract::{Query, State},
    http::{StatusCode, header},
    response::{Html, IntoResponse, Response},
};
use serde::Deserialize;
use tokio::sync::Mutex;

use crate::AppState;

const TTL_SECONDS: u64 = 600;
const CAPACITY: usize = 16;

struct PendingReturn {
    expires_at: u64,
    result: CentralLoginResult,
}

#[derive(Clone, Default)]
pub(crate) struct CentralLoginBroker {
    pending: Arc<Mutex<HashMap<String, PendingReturn>>>,
}

impl CentralLoginBroker {
    async fn control(
        &self,
        action: CentralLoginAction,
        state: &str,
        now: u64,
    ) -> Result<CentralLoginResult, &'static str> {
        if !(32..=128).contains(&state.len())
            || !state
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._:-".contains(&b))
        {
            return Err("invalid_login_state");
        }
        let mut pending = self.pending.lock().await;
        pending.retain(|_, value| value.expires_at > now);
        match action {
            CentralLoginAction::Start => {
                if pending.contains_key(state) {
                    return Err("login_state_exists");
                }
                if pending.len() >= CAPACITY {
                    return Err("login_capacity_exceeded");
                }
                let expires_at = now + TTL_SECONDS;
                let result = CentralLoginResult::Pending { expires_at };
                pending.insert(
                    state.to_owned(),
                    PendingReturn {
                        expires_at,
                        result: result.clone(),
                    },
                );
                Ok(result)
            }
            CentralLoginAction::Poll => pending
                .get(state)
                .map(|value| value.result.clone())
                .ok_or("login_expired"),
            CentralLoginAction::Cancel => {
                pending.remove(state);
                Ok(CentralLoginResult::Cancelled)
            }
        }
    }

    async fn complete(&self, callback: Callback, now: u64) -> Result<(), ()> {
        let result = match (callback.code, callback.error) {
            (Some(code), None)
                if (16..=2048).contains(&code.len())
                    && code.bytes().all(|b| b.is_ascii_graphic()) =>
            {
                CentralLoginResult::Complete {
                    authorization_code: code,
                }
            }
            (None, Some(error))
                if !error.is_empty()
                    && error.len() <= 128
                    && error.bytes().all(|b| b.is_ascii_graphic()) =>
            {
                CentralLoginResult::Failed
            }
            _ => return Err(()),
        };
        let mut pending = self.pending.lock().await;
        pending.retain(|_, value| value.expires_at > now);
        let value = pending.get_mut(&callback.state).ok_or(())?;
        match &value.result {
            CentralLoginResult::Pending { .. } => value.result = result,
            previous if previous == &result => (),
            _ => return Err(()),
        }
        Ok(())
    }
}

/// Operates only on the runtime's private native control pipe, before profile bootstrap.
pub async fn central_login_control(
    state: &AppState,
    request_id: String,
    action: CentralLoginAction,
    login_state: &str,
) -> LocalControlResponse {
    let result = if state.shutdown.is_cancelled() {
        Err("runtime_stopping")
    } else if let Ok(now) = unix_time() {
        state.central_login.control(action, login_state, now).await
    } else {
        Err("clock_unavailable")
    };
    match result {
        Ok(result) => LocalControlResponse::CentralLoginOk { request_id, result },
        Err(code) => LocalControlResponse::Error {
            request_id,
            code: code.into(),
            message: "Google login return is unavailable.".into(),
        },
    }
}

fn unix_time() -> Result<u64, ()> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|time| time.as_secs())
        .map_err(|_| ())
}

#[derive(Deserialize)]
struct Callback {
    state: String,
    code: Option<String>,
    error: Option<String>,
}

registered_routes! {
    pub(crate) fn routes<AppState>() {
        private "/api/central-login/callback" => get(callback),
        private "/central-login-complete" => get(complete_page),
    }
}

async fn callback(
    State(state): State<AppState>,
    query: Result<Query<Callback>, axum::extract::rejection::QueryRejection>,
) -> Response {
    let accepted = match (query, unix_time()) {
        (Ok(Query(query)), Ok(now)) if !state.shutdown.is_cancelled() => {
            state.central_login.complete(query, now).await.is_ok()
        }
        _ => false,
    };
    let response = if accepted {
        (
            StatusCode::SEE_OTHER,
            [(header::LOCATION, "/central-login-complete")],
        )
            .into_response()
    } else {
        (
            StatusCode::BAD_REQUEST,
            "This login return is invalid or expired.",
        )
            .into_response()
    };
    private_response(response)
}

async fn complete_page() -> Response {
    private_response(Html("<!doctype html><html lang=\"ko\"><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>AgentsAssemble</title><main><h1>앱으로 돌아가 주세요</h1><p>로그인 결과를 전달했습니다. 이 창을 닫아도 됩니다.</p></main></html>").into_response())
}

fn private_response(mut response: Response) -> Response {
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("private, no-store"),
    );
    response.headers_mut().insert(
        header::REFERRER_POLICY,
        axum::http::HeaderValue::from_static("no-referrer"),
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn return_is_bound_first_wins_and_cancel_retires_it() {
        let broker = CentralLoginBroker::default();
        let state = "a".repeat(43);
        let callback = |state: String, code: &str| Callback {
            state,
            code: Some(code.to_owned()),
            error: None,
        };
        assert!(
            broker
                .complete(callback(state.clone(), "first-code-123456"), 100)
                .await
                .is_err()
        );
        assert_eq!(
            broker.control(CentralLoginAction::Start, &state, 100).await,
            Ok(CentralLoginResult::Pending { expires_at: 700 })
        );
        assert!(
            broker
                .control(CentralLoginAction::Start, &state, 100)
                .await
                .is_err()
        );
        broker
            .complete(callback(state.clone(), "first-code-123456"), 101)
            .await
            .unwrap_or_else(|error| panic!("login fixture: {error:?}"));
        assert!(
            broker
                .complete(callback(state.clone(), "second-code-12345"), 101)
                .await
                .is_err()
        );
        assert_eq!(
            broker.control(CentralLoginAction::Poll, &state, 101).await,
            Ok(CentralLoginResult::Complete {
                authorization_code: "first-code-123456".into()
            })
        );
        broker
            .control(CentralLoginAction::Cancel, &state, 101)
            .await
            .unwrap_or_else(|error| panic!("login fixture: {error:?}"));
        assert!(
            broker
                .control(CentralLoginAction::Poll, &state, 101)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn capacity_expiry_and_provider_denial_are_explicit() {
        let broker = CentralLoginBroker::default();
        for n in 0..CAPACITY {
            broker
                .control(CentralLoginAction::Start, &format!("{n:043}"), 100)
                .await
                .unwrap_or_else(|error| panic!("login fixture: {error:?}"));
        }
        let state = "z".repeat(43);
        assert_eq!(
            broker.control(CentralLoginAction::Start, &state, 101).await,
            Err("login_capacity_exceeded")
        );
        broker
            .control(CentralLoginAction::Start, &state, 700)
            .await
            .unwrap_or_else(|error| panic!("login fixture: {error:?}"));
        broker
            .complete(
                Callback {
                    state: state.clone(),
                    code: None,
                    error: Some("access_denied".into()),
                },
                701,
            )
            .await
            .unwrap_or_else(|error| panic!("login fixture: {error:?}"));
        assert_eq!(
            broker.control(CentralLoginAction::Poll, &state, 701).await,
            Ok(CentralLoginResult::Failed)
        );
        assert!(
            broker
                .control(CentralLoginAction::Poll, &state, 1300)
                .await
                .is_err()
        );
    }
}
