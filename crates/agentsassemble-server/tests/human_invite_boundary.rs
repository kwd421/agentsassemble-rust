use agentsassemble_domain::InviteScope;
use agentsassemble_persistence::SqliteStore;
use base64::{
    Engine as _,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use reqwest::Client;
use serde_json::{Value, json};

mod support {
    pub mod human_invite;
    pub mod human_profile_exchange;
    pub mod subscription_proof;
}

use support::{
    human_invite::{
        canonical_session_token, fixture, fixture_with_max_uses, join, join_body,
        open_session_socket, persist_invite, start,
    },
    human_profile_exchange::{
        assert_avatar_available, assert_profile_exchange_boundary, issue_profile_ticket,
    },
};

#[tokio::test]
async fn preflight_and_join_preserve_bounded_credentials_and_exact_retry() {
    let (store, credentials) = fixture(InviteScope::ReadWrite).await;
    let server = start(store.clone()).await;
    let client = Client::new();
    let browser_credential = format!("aad1_{}", URL_SAFE_NO_PAD.encode([0xB7; 32]));
    let other_browser_credential = format!("aad1_{}", URL_SAFE_NO_PAD.encode([0xB8; 32]));

    assert_preflight_boundary(
        &client,
        &server.base_url,
        credentials.invite_token(),
        &browser_credential,
    )
    .await;
    let (avatar, other_browser_avatar) = prepare_prejoin_avatar_flow(
        &client,
        &server.base_url,
        credentials.join_code(),
        &browser_credential,
        &other_browser_credential,
    )
    .await;
    let request_id = "123e4567-e89b-12d3-a456-426614174000";
    let first = join(
        &client,
        &server.base_url,
        credentials.join_code(),
        &browser_credential,
        request_id,
        "Boundary Guest",
        &avatar,
    )
    .await;
    assert_eq!(first["status"], "admitted");
    assert_eq!(first["request_id"], request_id);
    assert_eq!(first["display_name"], "Boundary Guest");
    assert_eq!(first["meeting_id"], "general");
    assert_eq!(first["invite_scope"], "room");
    assert_eq!(first["client_type"], "browser");
    assert_eq!(first["participant_type"], "human");
    assert_eq!(first["avatar_image_url"], avatar);
    assert_session_server_surface(&first);
    let session_token = canonical_session_token(&first);
    assert_consumed_invite_preserves_live_session(
        &client,
        &server.base_url,
        credentials.join_code(),
        &browser_credential,
        session_token,
    )
    .await;
    let replacement_avatar =
        assert_profile_exchange_boundary(&client, &server.base_url, &store, session_token, &avatar)
            .await;
    assert_session_socket_boundary(&client, &server.base_url, session_token).await;

    let retry = join(
        &client,
        &server.base_url,
        credentials.join_code(),
        &browser_credential,
        request_id,
        "Boundary Guest",
        &avatar,
    )
    .await;
    assert!(retry == first, "exact retry response changed");
    assert_eq!(
        store
            .list_human_invites()
            .await
            .unwrap_or_else(|error| panic!("inspect invite after retry: {error}"))[0]
            .use_count,
        1
    );

    assert_changed_exact_join_conflicts(
        &client,
        &server.base_url,
        &store,
        credentials.join_code(),
        &browser_credential,
        request_id,
        &avatar,
    )
    .await;
    let replaced = client
        .get(format!("{}{}", server.base_url, avatar))
        .send()
        .await
        .unwrap_or_else(|error| panic!("read replaced admission avatar: {error}"));
    assert_eq!(replaced.status(), reqwest::StatusCode::NOT_FOUND);
    assert_avatar_available(&client, &server.base_url, &replacement_avatar).await;
    assert_avatar_available(&client, &server.base_url, &other_browser_avatar).await;

    server.stop().await;
}

#[tokio::test]
async fn read_only_session_patches_profile_but_cannot_upload_an_avatar() {
    let (store, credentials) = fixture(InviteScope::ReadOnly).await;
    let server = start(store).await;
    let client = Client::new();
    let browser_credential = format!("aad1_{}", URL_SAFE_NO_PAD.encode([0xC7; 32]));
    let admitted = join(
        &client,
        &server.base_url,
        credentials.join_code(),
        &browser_credential,
        "223e4567-e89b-12d3-a456-426614174000",
        "Read Only Guest",
        "",
    )
    .await;
    assert_eq!(admitted["invite_scope"], "read_only");
    let session_token = admitted["session_token"]
        .as_str()
        .unwrap_or_else(|| panic!("read-only admission has no session token"));

    let update_ticket = issue_profile_ticket(&client, &server.base_url, session_token).await;
    let updated: Value = client
        .post(format!("{}/api/user-profile", server.base_url))
        .header("authorization", format!("Bearer {update_ticket}"))
        .json(&json!({
            "expected_revision": 1,
            "custom_status": "Still a person profile"
        }))
        .send()
        .await
        .unwrap_or_else(|error| panic!("patch read-only person profile: {error}"))
        .json()
        .await
        .unwrap_or_else(|error| panic!("decode read-only person profile: {error}"));
    assert_eq!(
        updated["profile"]["custom_status"],
        "Still a person profile"
    );

    let upload_ticket = issue_profile_ticket(&client, &server.base_url, session_token).await;
    let rejected = client
        .post(format!("{}/api/attachments", server.base_url))
        .header("authorization", format!("Bearer {upload_ticket}"))
        .body("{")
        .send()
        .await
        .unwrap_or_else(|error| panic!("reject read-only avatar upload: {error}"));
    assert_eq!(rejected.status(), reqwest::StatusCode::FORBIDDEN);
    let rejected: Value = rejected
        .json()
        .await
        .unwrap_or_else(|error| panic!("decode read-only upload rejection: {error}"));
    assert_eq!(rejected["code"], "session_read_only");

    let mut socket = open_session_socket(&client, &server.base_url, session_token).await;
    socket
        .send_json(&json!({
            "op": "command",
            "request_id": "read-only-message-1",
            "action": "message.send",
            "payload": {"content": "must not commit"}
        }))
        .await;
    let nack = socket.receive_json().await;
    assert_eq!(nack["op"], "nack");
    assert_eq!(nack["error"]["code"], "permission_denied");
    socket.close().await;
    server.stop().await;
}

#[tokio::test]
async fn remote_preferences_use_exact_session_purposes_and_revalidate_durable_authority() {
    let client = Client::new();
    assert_writable_remote_preferences(&client).await;
    assert_read_only_remote_preferences(&client).await;
}

async fn assert_writable_remote_preferences(client: &Client) {
    let (store, credentials) = fixture_with_max_uses(InviteScope::ReadWrite, 5).await;
    let replacement = persist_invite(
        &store,
        InviteScope::ReadWrite,
        5,
        "preference-replacement-guest",
        "Preference Replacement Guest",
    )
    .await;
    let server = start(store).await;
    let browser_credential = format!("aad1_{}", URL_SAFE_NO_PAD.encode([0xE7; 32]));
    let admitted = join(
        client,
        &server.base_url,
        credentials.join_code(),
        &browser_credential,
        "523e4567-e89b-12d3-a456-426614174000",
        "Preference Guest",
        "",
    )
    .await;
    let session_token = canonical_session_token(&admitted);

    let write_ticket = issue_session_preference_ticket(
        client,
        &server.base_url,
        session_token,
        "preferences-write",
    )
    .await;
    let updated = client
        .post(format!("{}/api/room-settings", server.base_url))
        .bearer_auth(&write_ticket)
        .json(&json!({
            "room_id": "general",
            "appearance": {"notifications": "mute"}
        }))
        .send()
        .await
        .unwrap_or_else(|error| panic!("write remote preferences: {error}"));
    assert_eq!(updated.status(), reqwest::StatusCode::OK);
    let updated: Value = updated
        .json()
        .await
        .unwrap_or_else(|error| panic!("decode remote preferences: {error}"));
    assert_eq!(updated["settings"]["appearance"]["notifications"], "mute");
    let replay = client
        .post(format!("{}/api/room-settings", server.base_url))
        .bearer_auth(write_ticket)
        .json(&json!({"room_id": "general"}))
        .send()
        .await
        .unwrap_or_else(|error| panic!("replay remote preference write: {error}"));
    assert_eq!(replay.status(), reqwest::StatusCode::UNAUTHORIZED);

    let read_ticket = issue_session_preference_ticket(
        client,
        &server.base_url,
        session_token,
        "preferences-read",
    )
    .await;
    let cross_room = client
        .get(format!(
            "{}/api/room-settings?room_id=other",
            server.base_url
        ))
        .bearer_auth(&read_ticket)
        .send()
        .await
        .unwrap_or_else(|error| panic!("cross remote preference room: {error}"));
    assert_eq!(cross_room.status(), reqwest::StatusCode::UNAUTHORIZED);
    let consumed = client
        .get(format!(
            "{}/api/room-settings?room_id=general",
            server.base_url
        ))
        .bearer_auth(read_ticket)
        .send()
        .await
        .unwrap_or_else(|error| panic!("replay crossed preference ticket: {error}"));
    assert_eq!(consumed.status(), reqwest::StatusCode::UNAUTHORIZED);

    assert_replaced_session_preference_fails(
        client,
        &server.base_url,
        session_token,
        replacement.join_code(),
        &browser_credential,
    )
    .await;
    server.stop().await;
}

async fn assert_replaced_session_preference_fails(
    client: &Client,
    base_url: &str,
    session_token: &str,
    replacement_join_code: &str,
    browser_credential: &str,
) {
    let stale_ticket =
        issue_session_preference_ticket(client, base_url, session_token, "preferences-read").await;
    join(
        client,
        base_url,
        replacement_join_code,
        browser_credential,
        "623e4567-e89b-12d3-a456-426614174000",
        "Preference Replacement Guest",
        "",
    )
    .await;
    let stale = client
        .get(format!("{base_url}/api/room-settings?room_id=general"))
        .bearer_auth(stale_ticket)
        .send()
        .await
        .unwrap_or_else(|error| panic!("consume replaced-session preference ticket: {error}"));
    assert_eq!(stale.status(), reqwest::StatusCode::UNAUTHORIZED);
}

async fn assert_read_only_remote_preferences(client: &Client) {
    let (read_only_store, read_only_credentials) = fixture(InviteScope::ReadOnly).await;
    let read_only_server = start(read_only_store).await;
    let read_only = join(
        client,
        &read_only_server.base_url,
        read_only_credentials.join_code(),
        &format!("aad1_{}", URL_SAFE_NO_PAD.encode([0xF7; 32])),
        "723e4567-e89b-12d3-a456-426614174000",
        "Read Only Preference Guest",
        "",
    )
    .await;
    let read_only_session = canonical_session_token(&read_only);
    let read_ticket = issue_session_preference_ticket(
        client,
        &read_only_server.base_url,
        read_only_session,
        "preferences-read",
    )
    .await;
    let readable = client
        .get(format!(
            "{}/api/room-settings?room_id=general",
            read_only_server.base_url
        ))
        .bearer_auth(read_ticket)
        .send()
        .await
        .unwrap_or_else(|error| panic!("read read-only preferences: {error}"));
    assert_eq!(readable.status(), reqwest::StatusCode::OK);
    let readable: Value = readable
        .json()
        .await
        .unwrap_or_else(|error| panic!("decode read-only preferences: {error}"));
    assert_eq!(
        readable["settings"]["appearance"]["notifications"],
        "mentions"
    );
    let denied = client
        .post(format!(
            "{}/api/session-tickets/preferences-write",
            read_only_server.base_url
        ))
        .bearer_auth(read_only_session)
        .send()
        .await
        .unwrap_or_else(|error| panic!("deny read-only preference exchange: {error}"));
    assert_eq!(denied.status(), reqwest::StatusCode::FORBIDDEN);
    let denied: Value = denied
        .json()
        .await
        .unwrap_or_else(|error| panic!("decode read-only preference denial: {error}"));
    assert_eq!(denied["code"], "session_read_only");
    read_only_server.stop().await;
}

#[tokio::test]
async fn replacement_admission_closes_an_idle_human_socket() {
    let (store, first_invite) = fixture_with_max_uses(InviteScope::ReadWrite, 5).await;
    let second_invite = persist_invite(
        &store,
        InviteScope::ReadWrite,
        5,
        "replacement-boundary-guest",
        "Replacement Boundary Guest",
    )
    .await;
    let server = start(store).await;
    let client = Client::new();
    let browser_credential = format!("aad1_{}", URL_SAFE_NO_PAD.encode([0xD7; 32]));
    let first = join(
        &client,
        &server.base_url,
        first_invite.join_code(),
        &browser_credential,
        "323e4567-e89b-12d3-a456-426614174000",
        "First Reusable Guest",
        "",
    )
    .await;
    let first_session = canonical_session_token(&first);
    let mut socket = open_session_socket(&client, &server.base_url, first_session).await;

    let second = join(
        &client,
        &server.base_url,
        second_invite.join_code(),
        &browser_credential,
        "423e4567-e89b-12d3-a456-426614174000",
        "Replacement Reusable Guest",
        "",
    )
    .await;
    assert_ne!(canonical_session_token(&second), first_session);
    assert!(
        socket.wait_closed().await,
        "replaced idle socket stayed open"
    );

    let rejected = client
        .post(format!("{}/api/session-tickets/socket", server.base_url))
        .header("authorization", format!("Bearer {first_session}"))
        .send()
        .await
        .unwrap_or_else(|error| panic!("recheck replaced session exchange: {error}"));
    assert_eq!(rejected.status(), reqwest::StatusCode::UNAUTHORIZED);
    server.stop().await;
}

async fn assert_session_socket_boundary(client: &Client, base_url: &str, session_token: &str) {
    let mut socket = open_session_socket(client, base_url, session_token).await;
    socket
        .send_json(&json!({
            "op": "command",
            "request_id": "human-socket-message-1",
            "action": "message.send",
            "payload": {"content": "human socket boundary"}
        }))
        .await;
    let first = socket.receive_json().await;
    let second = socket.receive_json().await;
    assert!(
        [&first, &second].iter().any(|frame| {
            frame["op"] == "ack" && frame["request_id"] == "human-socket-message-1"
        })
    );
    assert!([&first, &second].iter().any(|frame| {
        frame["op"] == "event"
            && frame["events"].as_array().is_some_and(|events| {
                events
                    .iter()
                    .any(|event| event["content"] == "human socket boundary")
            })
    }));
    socket.close().await;
}

async fn issue_session_preference_ticket(
    client: &Client,
    base_url: &str,
    session_token: &str,
    purpose: &str,
) -> String {
    let response = client
        .post(format!("{base_url}/api/session-tickets/{purpose}"))
        .bearer_auth(session_token)
        .send()
        .await
        .unwrap_or_else(|error| panic!("exchange human preference ticket: {error}"));
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    let grant: Value = response
        .json()
        .await
        .unwrap_or_else(|error| panic!("decode human preference ticket: {error}"));
    let ticket = grant["ticket"]
        .as_str()
        .unwrap_or_else(|| panic!("human preference ticket is missing"));
    assert_eq!(ticket.len(), 64);
    assert!(ticket.bytes().all(|byte| byte.is_ascii_hexdigit()));
    ticket.to_owned()
}

fn assert_session_server_surface(admission: &Value) {
    for field in ["server_id", "authority_lineage_id"] {
        assert!(
            admission[field]
                .as_str()
                .is_some_and(|value| uuid::Uuid::parse_str(value).is_ok()),
            "{field} is not a UUID"
        );
    }
    assert_eq!(
        admission["server_product_surface"]["websocket_streams"],
        json!(["room_events"])
    );
}

async fn prepare_prejoin_avatar_flow(
    client: &Client,
    base_url: &str,
    invite_token: &str,
    browser_credential: &str,
    other_browser_credential: &str,
) -> (String, String) {
    let invalid_ticket = client
        .post(format!("{base_url}/api/attachments"))
        .header("authorization", "Bearer invalid-profile-ticket")
        .body("{")
        .send()
        .await
        .unwrap_or_else(|error| panic!("request prejoin upload with invalid ticket: {error}"));
    assert_eq!(invalid_ticket.status(), reqwest::StatusCode::UNAUTHORIZED);

    let unknown_join_code = format!("aaj1_{}", URL_SAFE_NO_PAD.encode([0xE1; 24]));
    let large_valid_base64 = STANDARD.encode(vec![0_u8; 10 * 1024 * 1024]);
    let unknown_invite = client
        .post(format!("{base_url}/api/attachments"))
        .json(&json!({
            "purpose": "profile_avatar",
            "invite_token": unknown_join_code,
            "device_token": browser_credential,
            "filename": "unknown.png",
            "content_type": "image/png",
            "data_base64": large_valid_base64,
        }))
        .send()
        .await
        .unwrap_or_else(|error| panic!("request unknown-invite prejoin upload: {error}"));
    assert_eq!(unknown_invite.status(), reqwest::StatusCode::FORBIDDEN);
    let unknown_invite: Value = unknown_invite
        .json()
        .await
        .unwrap_or_else(|error| panic!("decode unknown-invite prejoin rejection: {error}"));
    assert_eq!(unknown_invite["code"], "invite_invalid");

    let other_browser_avatar =
        upload_prejoin_avatar(client, base_url, invite_token, other_browser_credential).await;
    let replaced_avatar =
        upload_prejoin_avatar(client, base_url, invite_token, browser_credential).await;
    let avatar = upload_prejoin_avatar(client, base_url, invite_token, browser_credential).await;
    let replaced = client
        .get(format!("{base_url}{replaced_avatar}"))
        .send()
        .await
        .unwrap_or_else(|error| panic!("read replaced prejoin avatar: {error}"));
    assert_eq!(replaced.status(), reqwest::StatusCode::NOT_FOUND);
    assert_avatar_available(client, base_url, &avatar).await;
    assert_avatar_available(client, base_url, &other_browser_avatar).await;
    (avatar, other_browser_avatar)
}

async fn assert_preflight_boundary(
    client: &Client,
    base_url: &str,
    invite_token: &str,
    browser_credential: &str,
) {
    let route = format!("{base_url}/api/room-invite/admission");
    let missing_browser = client
        .post(&route)
        .json(&json!({"invite_token": invite_token}))
        .send()
        .await
        .unwrap_or_else(|error| panic!("request preflight without browser credential: {error}"));
    assert_eq!(missing_browser.status(), reqwest::StatusCode::BAD_REQUEST);

    let invalid_invite: Value = client
        .post(&route)
        .header("x-device-token", browser_credential)
        .json(&json!({"invite_token": "not-an-invite"}))
        .send()
        .await
        .unwrap_or_else(|error| panic!("request invalid invite preflight: {error}"))
        .json()
        .await
        .unwrap_or_else(|error| panic!("decode invalid invite preflight: {error}"));
    assert_eq!(invalid_invite["status"], "invite_invalid");
    assert_eq!(invalid_invite["can_auto_join"], false);

    let preflight = client
        .post(&route)
        .header("origin", "tauri://localhost")
        .header("x-device-token", browser_credential)
        .json(&json!({"invite_token": invite_token}))
        .send()
        .await
        .unwrap_or_else(|error| panic!("request invite preflight: {error}"));
    assert_eq!(preflight.status(), reqwest::StatusCode::OK);
    assert_eq!(
        preflight.headers()["access-control-allow-origin"],
        "tauri://localhost"
    );
    assert_eq!(preflight.headers()["cache-control"], "private, no-store");
    let preflight: Value = preflight
        .json()
        .await
        .unwrap_or_else(|error| panic!("decode invite preflight: {error}"));
    assert_eq!(preflight["status"], "profile_required");
    assert_eq!(preflight["room_id"], "general");
    assert_eq!(preflight["room_label"], "General");
    assert_eq!(preflight["invite_scope"], "room");
    assert_eq!(preflight["can_auto_join"], false);

    let cors = client
        .request(reqwest::Method::OPTIONS, &route)
        .header("origin", "tauri://localhost")
        .header("access-control-request-method", "POST")
        .header(
            "access-control-request-headers",
            "authorization,content-type,x-device-token",
        )
        .send()
        .await
        .unwrap_or_else(|error| panic!("request invite CORS preflight: {error}"));
    assert!(cors.status().is_success());
    let allowed = cors.headers()["access-control-allow-headers"]
        .to_str()
        .unwrap_or_else(|error| panic!("decode invite CORS headers: {error}"));
    assert!(allowed.contains("authorization"));
    assert!(allowed.contains("content-type"));
    assert!(allowed.contains("x-device-token"));
}

async fn assert_consumed_invite_preserves_live_session(
    client: &Client,
    base_url: &str,
    invite_token: &str,
    browser_credential: &str,
    session_token: &str,
) {
    let existing: Value = client
        .post(format!("{base_url}/api/room-invite/admission"))
        .bearer_auth(session_token)
        .header("x-device-token", browser_credential)
        .json(&json!({"invite_token": invite_token}))
        .send()
        .await
        .unwrap_or_else(|error| panic!("preflight consumed invite with live session: {error}"))
        .json()
        .await
        .unwrap_or_else(|error| panic!("decode live-session preflight: {error}"));
    assert_eq!(existing["status"], "existing_session");
    assert_eq!(existing["invite_scope"], "room");
    assert_eq!(existing["can_auto_join"], true);
}

async fn assert_changed_exact_join_conflicts(
    client: &Client,
    base_url: &str,
    store: &SqliteStore,
    invite_token: &str,
    browser_credential: &str,
    request_id: &str,
    avatar: &str,
) {
    let conflict = client
        .post(format!("{base_url}/api/room-invite/join"))
        .json(&join_body(
            invite_token,
            browser_credential,
            request_id,
            "Changed Guest",
            avatar,
        ))
        .send()
        .await
        .unwrap_or_else(|error| panic!("request conflicting exact join: {error}"));
    assert_eq!(conflict.status(), reqwest::StatusCode::CONFLICT);
    let conflict: Value = conflict
        .json()
        .await
        .unwrap_or_else(|error| panic!("decode conflicting exact join: {error}"));
    assert_eq!(conflict["code"], "idempotency_conflict");
    assert_eq!(
        store
            .list_human_invites()
            .await
            .unwrap_or_else(|error| panic!("inspect invite after conflict: {error}"))[0]
            .use_count,
        1
    );
}

async fn upload_prejoin_avatar(
    client: &Client,
    base_url: &str,
    invite_token: &str,
    browser_credential: &str,
) -> String {
    let response = client
        .post(format!("{base_url}/api/attachments"))
        .json(&json!({
            "purpose": "profile_avatar",
            "invite_token": invite_token,
            "device_token": browser_credential,
            "filename": "../guest.webp",
            "content_type": "image/png",
            "data_base64": "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGMQ0bD5DwACRAF4aig0hQAAAABJRU5ErkJggg=="
        }))
        .send()
        .await
        .unwrap_or_else(|error| panic!("upload prejoin avatar: {error}"));
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let response: Value = response
        .json()
        .await
        .unwrap_or_else(|error| panic!("decode prejoin avatar upload: {error}"));
    assert_eq!(response["attachment"]["filename"], "guest.png");
    assert_eq!(response["attachment"]["content_type"], "image/png");
    response["attachment"]["url"]
        .as_str()
        .unwrap_or_else(|| panic!("prejoin avatar URL is missing"))
        .to_owned()
}
