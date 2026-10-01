use agentsassemble_persistence::SqliteStore;
use agentsassemble_protocol::{CentralLoginAction, CentralLoginResult, LocalControlResponse};
use agentsassemble_server::central_login_control;
use reqwest::{Client, StatusCode};

mod support {
    pub mod human_invite;
    pub mod room_socket_peer;
}

#[tokio::test]
async fn native_return_before_bootstrap_requires_expected_state_and_local_ingress() {
    let store = SqliteStore::open("sqlite::memory:")
        .await
        .unwrap_or_else(|error| panic!("store: {error}"));
    let server = support::human_invite::start(store).await;
    let client = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap_or_else(|error| panic!("client: {error}"));
    let state = "s".repeat(43);
    let response = central_login_control(
        server.state(),
        "start".into(),
        CentralLoginAction::Start,
        &state,
    )
    .await;
    assert!(matches!(
        response,
        LocalControlResponse::CentralLoginOk {
            result: CentralLoginResult::Pending { .. },
            ..
        }
    ));
    let callback = format!("{}/api/central-login/callback", server.base_url);
    for (header, value) in [
        ("origin", "https://attacker.example"),
        ("x-forwarded-host", "attacker.example"),
        ("host", "attacker.example"),
    ] {
        let response = client
            .get(format!("{callback}?state={state}&code=fixture-code-123456"))
            .header(header, value)
            .send()
            .await
            .unwrap_or_else(|error| panic!("request: {error}"));
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
    let response = client
        .get(format!("{callback}?state=unknown&code=fixture-code-123456"))
        .send()
        .await
        .unwrap_or_else(|error| panic!("request: {error}"));
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let failed = response
        .text()
        .await
        .unwrap_or_else(|error| panic!("failed page: {error}"));
    assert!(failed.contains("data-result=\"failed\""), "{failed}");
    let response = client
        .get(format!("{callback}?state={state}&code=fixture-code-123456"))
        .send()
        .await
        .unwrap_or_else(|error| panic!("request: {error}"));
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(response.headers()["location"], "/central-login-complete");
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    assert_eq!(response.headers()["referrer-policy"], "no-referrer");
    let page = client
        .get(format!("{}/central-login-complete", server.base_url))
        .send()
        .await
        .unwrap_or_else(|error| panic!("complete page: {error}"));
    assert_eq!(page.status(), StatusCode::OK);
    let page = page
        .text()
        .await
        .unwrap_or_else(|error| panic!("complete page body: {error}"));
    assert!(page.contains("로그인 완료"), "{page}");
    // The app-wide CSP allows only same-origin styles, so the page links its sheet.
    assert!(page.contains(r#"href="/central-login.css""#), "{page}");
    assert!(!page.contains("<style") && !page.contains("style=\""), "{page}");
    let style = client
        .get(format!("{}/central-login.css", server.base_url))
        .send()
        .await
        .unwrap_or_else(|error| panic!("page style: {error}"));
    assert_eq!(style.status(), StatusCode::OK);
    assert_eq!(style.headers()["content-type"], "text/css; charset=utf-8");
    assert_eq!(
        central_login_control(
            server.state(),
            "poll".into(),
            CentralLoginAction::Poll,
            &state
        )
        .await,
        LocalControlResponse::CentralLoginOk {
            request_id: "poll".into(),
            result: CentralLoginResult::Complete {
                authorization_code: "fixture-code-123456".into()
            }
        }
    );
    server.stop().await;
}
