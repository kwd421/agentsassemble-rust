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

/// Transient OAuth return authority, independent of room storage and providers.
#[derive(Clone, Default)]
pub struct CentralLoginService {
    broker: CentralLoginBroker,
    shutdown: tokio_util::sync::CancellationToken,
}

impl From<&AppState> for CentralLoginService {
    fn from(state: &AppState) -> Self {
        Self {
            broker: state.central_login.clone(),
            shutdown: state.shutdown.clone(),
        }
    }
}

impl CentralLoginService {
    /// Operates only on the owned process's private control pipe.
    pub async fn control(
        &self,
        request_id: String,
        action: CentralLoginAction,
        login_state: &str,
    ) -> LocalControlResponse {
        let result = if self.shutdown.is_cancelled() {
            Err("runtime_stopping")
        } else if let Ok(now) = unix_time() {
            self.broker.control(action, login_state, now).await
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

    /// Exposes only the callback and completion page, at the exact owned authority.
    pub fn router(self, authority: String) -> axum::Router {
        routes().with_state(self).layer(axum::middleware::from_fn(
            move |request: axum::extract::Request, next: axum::middleware::Next| {
                let authority = authority.clone();
                async move {
                    let hosts = request.headers().get_all(header::HOST);
                    let mut hosts = hosts.iter();
                    let valid = hosts
                        .next()
                        .is_some_and(|host| host.as_bytes() == authority.as_bytes())
                        && hosts.next().is_none()
                        && !request.headers().contains_key("forwarded")
                        && !request
                            .headers()
                            .keys()
                            .any(|key| key.as_str().starts_with("x-forwarded-"))
                        && request
                            .headers()
                            .get_all(header::ORIGIN)
                            .iter()
                            .all(|origin| {
                                origin.as_bytes() == format!("http://{authority}").as_bytes()
                            });
                    let mut response = if valid {
                        next.run(request).await
                    } else {
                        StatusCode::BAD_REQUEST.into_response()
                    };
                    response.headers_mut().insert(
                        header::CONTENT_SECURITY_POLICY,
                        crate::security_headers::content_security_policy(false),
                    );
                    crate::security_headers::apply(private_response(response)).await
                }
            },
        ))
    }
}

pub async fn central_login_control(
    state: &AppState,
    request_id: String,
    action: CentralLoginAction,
    login_state: &str,
) -> LocalControlResponse {
    CentralLoginService::from(state)
        .control(request_id, action, login_state)
        .await
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
    pub(crate) fn routes<CentralLoginService>() {
        private "/api/central-login/callback" => get(callback),
        private "/central-login-complete" => get(complete_page),
        private "/central-login.css" => get(page_style),
    }
}

async fn callback(
    State(state): State<CentralLoginService>,
    query: Result<Query<Callback>, axum::extract::rejection::QueryRejection>,
) -> Response {
    let accepted = match (query, unix_time()) {
        (Ok(Query(query)), Ok(now)) if !state.shutdown.is_cancelled() => {
            state.broker.complete(query, now).await.is_ok()
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
            Html(result_page(
                "failed",
                FAILED_MARK,
                "로그인을 마치지 못했어요",
                "로그인 요청이 만료됐거나 올바르지 않아요. 앱에서 다시 시도해 주세요.",
            )),
        )
            .into_response()
    };
    private_response(response)
}

async fn complete_page() -> Response {
    private_response(
        Html(result_page(
            "done",
            DONE_MARK,
            "로그인 완료",
            "AgentsAssemble 앱으로 돌아가 주세요. 이 창은 닫아도 됩니다.",
        ))
        .into_response(),
    )
}

// The app-wide CSP allows only same-origin styles, so the page links its sheet.
async fn page_style() -> Response {
    private_response(
        (
            [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
            PAGE_STYLE,
        )
            .into_response(),
    )
}

const DONE_MARK: &str = r#"<path d="M6 12.5l4 4L18 8"/>"#;
const FAILED_MARK: &str = r#"<path d="M12 7v6M12 16.5v.5"/>"#;

fn result_page(result: &str, mark: &str, title: &str, body: &str) -> String {
    format!(
        r#"<!doctype html><html lang="ko"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>AgentsAssemble</title><link rel="stylesheet" href="/central-login.css"></head><body><main class="card" data-result="{result}"><div class="mark" aria-hidden="true"><svg viewBox="0 0 24 24">{mark}</svg></div><p class="app">AgentsAssemble</p><h1>{title}</h1><p>{body}</p></main></body></html>"#
    )
}

const PAGE_STYLE: &str = r#"*{box-sizing:border-box}
html,body{height:100%;margin:0}
body{display:grid;place-items:center;padding:16px;background:#1a1b20;color:#f2f3f5;font-family:"Pretendard Variable",Pretendard,"Apple SD Gothic Neo","Malgun Gothic",system-ui,sans-serif;word-break:keep-all}
.card{width:min(400px,100%);padding:36px 32px 32px;border:1px solid rgba(255,255,255,.08);border-radius:12px;background:#202127;text-align:center;box-shadow:0 8px 24px rgb(0 0 0/24%)}
.mark{display:grid;width:56px;height:56px;margin:0 auto 18px;place-items:center;border-radius:50%;background:#23a55a}
[data-result=failed] .mark{background:#f23f42}
.mark svg{width:30px;height:30px;fill:none;stroke:#fff;stroke-width:2.6;stroke-linecap:round;stroke-linejoin:round}
.app{margin:0 0 6px;color:#949ba4;font-size:12px;font-weight:700;letter-spacing:.04em}
h1{margin:0 0 10px;font-size:22px;font-weight:800}
p{margin:0;color:#b5bac1;font-size:14px;line-height:1.6}
"#;

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
