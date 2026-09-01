use agentsassemble_domain::{
    AuthenticatedPrincipal, CapabilitySet, ClientKind, InviteScope, LOCAL_OPERATOR_PARTICIPANT_ID,
    LOCAL_OPERATOR_USER_ID, RoomSettings, public_settings,
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use reqwest::Client;
use serde_json::{Value, json};

mod support {
    pub mod human_invite;
    pub mod room_socket_peer;
}

use support::human_invite::{canonical_session_token, fixture, join, start};

#[tokio::test]
async fn human_session_exchanges_one_exact_bound_appearance_read() {
    let (store, credentials) = fixture(InviteScope::ReadOnly).await;
    let manager = store
        .authorize_local_room_manager(
            "general",
            LOCAL_OPERATOR_USER_ID,
            LOCAL_OPERATOR_PARTICIPANT_ID,
        )
        .await
        .unwrap_or_else(|error| panic!("authorize appearance manager: {error}"));
    let stored = store
        .store_pending_room_appearance_asset(
            &manager,
            "remote-room-icon.png",
            "image/png",
            base64::engine::general_purpose::STANDARD
                .decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGMQ0bD5DwACRAF4aig0hQAAAABJRU5ErkJggg==")
                .unwrap_or_else(|error| panic!("decode appearance fixture: {error}")),
        )
        .await
        .unwrap_or_else(|error| panic!("store appearance fixture: {error}"));
    let revision = public_settings(&RoomSettings::defaults("General"))
        .unwrap_or_else(|error| panic!("read appearance revision: {error}"))
        .settings_revision;
    store
        .execute_room_settings_update(
            &local_principal(),
            "remote-appearance-bind",
            &json!({
                "expected_revision": revision,
                "appearance": {"icon_image_url": stored.url}
            }),
        )
        .await
        .unwrap_or_else(|error| panic!("bind remote appearance fixture: {error}"));

    let server = start(store).await;
    let client = Client::new();
    let browser_credential = format!("aad1_{}", URL_SAFE_NO_PAD.encode([0x39; 32]));
    let admitted = join(
        &client,
        &server.base_url,
        credentials.join_code(),
        &browser_credential,
        "823e4567-e89b-12d3-a456-426614174000",
        "Appearance Reader",
        "",
    )
    .await;
    let session_token = canonical_session_token(&admitted);

    let raw_session = client
        .get(format!("{}{}", server.base_url, stored.url))
        .bearer_auth(session_token)
        .send()
        .await
        .unwrap_or_else(|error| panic!("reject raw session appearance read: {error}"));
    assert_eq!(raw_session.status(), reqwest::StatusCode::UNAUTHORIZED);
    let invalid = client
        .post(format!(
            "{}/api/session-tickets/room-appearance/profile_avatar",
            server.base_url
        ))
        .bearer_auth(session_token)
        .send()
        .await
        .unwrap_or_else(|error| panic!("reject malformed appearance exchange: {error}"));
    assert_eq!(invalid.status(), reqwest::StatusCode::BAD_REQUEST);

    let ticket =
        issue_appearance_ticket(&client, &server.base_url, session_token, &stored.id).await;
    let readable = client
        .get(format!("{}{}", server.base_url, stored.url))
        .bearer_auth(&ticket)
        .send()
        .await
        .unwrap_or_else(|error| panic!("read exchanged room appearance: {error}"));
    assert_private_png(readable).await;
    let replay = client
        .get(format!("{}{}", server.base_url, stored.url))
        .bearer_auth(ticket)
        .send()
        .await
        .unwrap_or_else(|error| panic!("reject appearance ticket replay: {error}"));
    assert_eq!(replay.status(), reqwest::StatusCode::UNAUTHORIZED);

    let stale = issue_appearance_ticket(&client, &server.base_url, session_token, &stored.id).await;
    let leave = client
        .post(format!("{}/api/room-invite/leave", server.base_url))
        .bearer_auth(session_token)
        .json(&json!({}))
        .send()
        .await
        .unwrap_or_else(|error| panic!("leave appearance room: {error}"));
    assert_eq!(leave.status(), reqwest::StatusCode::OK);
    let revoked = client
        .get(format!("{}{}", server.base_url, stored.url))
        .bearer_auth(stale)
        .send()
        .await
        .unwrap_or_else(|error| panic!("reject revoked appearance session: {error}"));
    assert_eq!(revoked.status(), reqwest::StatusCode::UNAUTHORIZED);
    server.stop().await;
}

async fn issue_appearance_ticket(
    client: &Client,
    base_url: &str,
    session_token: &str,
    asset_id: &str,
) -> String {
    let response = client
        .post(format!(
            "{base_url}/api/session-tickets/room-appearance/{asset_id}"
        ))
        .bearer_auth(session_token)
        .send()
        .await
        .unwrap_or_else(|error| panic!("exchange room appearance ticket: {error}"));
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    response
        .json::<Value>()
        .await
        .unwrap_or_else(|error| panic!("decode room appearance ticket: {error}"))["ticket"]
        .as_str()
        .unwrap_or_else(|| panic!("room appearance ticket is missing"))
        .to_owned()
}

async fn assert_private_png(response: reqwest::Response) {
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "image/png");
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    assert_eq!(response.headers()["x-content-type-options"], "nosniff");
    assert!(
        response
            .bytes()
            .await
            .unwrap_or_default()
            .starts_with(b"\x89PNG\r\n\x1a\n")
    );
}

fn local_principal() -> AuthenticatedPrincipal {
    AuthenticatedPrincipal {
        principal_id: LOCAL_OPERATOR_USER_ID.to_owned(),
        participant_id: LOCAL_OPERATOR_PARTICIPANT_ID.to_owned(),
        display_name: "SeiNel".to_owned(),
        room_id: "general".to_owned(),
        client_kind: ClientKind::Browser,
        invite_scope: InviteScope::ReadWrite,
        is_operator: true,
        capabilities: CapabilitySet::local_operator(ClientKind::Browser, InviteScope::ReadWrite),
    }
}
