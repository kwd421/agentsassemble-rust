use agentsassemble_persistence::SqliteStore;
use reqwest::Client;
use serde_json::{Value, json};

pub(crate) async fn assert_profile_exchange_boundary(
    client: &Client,
    base_url: &str,
    store: &SqliteStore,
    session_token: &str,
    admitted_avatar: &str,
) -> String {
    let raw_target = client
        .get(format!("{base_url}/api/user-profile"))
        .header("authorization", format!("Bearer {session_token}"))
        .send()
        .await
        .unwrap_or_else(|error| panic!("reject raw session at profile target: {error}"));
    assert_eq!(raw_target.status(), reqwest::StatusCode::UNAUTHORIZED);
    assert_eq!(raw_target.headers()["cache-control"], "private, no-store");

    let nonempty = client
        .post(format!("{base_url}/api/session-tickets/profile"))
        .header("authorization", format!("Bearer {session_token}"))
        .body("{}")
        .send()
        .await
        .unwrap_or_else(|error| panic!("reject nonempty profile exchange: {error}"));
    assert_eq!(nonempty.status(), reqwest::StatusCode::BAD_REQUEST);
    assert_eq!(nonempty.headers()["cache-control"], "private, no-store");

    let read_ticket = issue_profile_ticket(client, base_url, session_token).await;
    let profile = client
        .get(format!("{base_url}/api/user-profile"))
        .header("authorization", format!("Bearer {read_ticket}"))
        .send()
        .await
        .unwrap_or_else(|error| panic!("read exchanged profile: {error}"));
    assert_eq!(profile.headers()["cache-control"], "private, no-store");
    let profile: Value = profile
        .json()
        .await
        .unwrap_or_else(|error| panic!("decode exchanged profile: {error}"));
    assert_eq!(profile["profile"]["display_name"], "Boundary Guest");
    assert_eq!(profile["profile"]["avatar_image_url"], admitted_avatar);
    let replay = client
        .get(format!("{base_url}/api/user-profile"))
        .header("authorization", format!("Bearer {read_ticket}"))
        .send()
        .await
        .unwrap_or_else(|error| panic!("reject profile ticket replay: {error}"));
    assert_eq!(replay.status(), reqwest::StatusCode::UNAUTHORIZED);

    let upload_ticket = issue_profile_ticket(client, base_url, session_token).await;
    let upload: Value = client
        .post(format!("{base_url}/api/attachments"))
        .header("authorization", format!("Bearer {upload_ticket}"))
        .json(&json!({
            "purpose": "profile_avatar",
            "filename": "replacement.png",
            "content_type": "image/png",
            "data_base64": "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGMQ0bD5DwACRAF4aig0hQAAAABJRU5ErkJggg=="
        }))
        .send()
        .await
        .unwrap_or_else(|error| panic!("upload exchanged profile avatar: {error}"))
        .json()
        .await
        .unwrap_or_else(|error| panic!("decode exchanged profile avatar: {error}"));
    let replacement_avatar = upload["attachment"]["url"]
        .as_str()
        .unwrap_or_else(|| panic!("exchanged profile avatar URL is missing"));

    let update_ticket = issue_profile_ticket(client, base_url, session_token).await;
    let updated: Value = client
        .post(format!("{base_url}/api/user-profile"))
        .header("authorization", format!("Bearer {update_ticket}"))
        .json(&json!({
            "expected_revision": 1,
            "display_name": "Exchanged Guest",
            "avatar_image_url": replacement_avatar,
            "mic_muted": true,
            "deafened": true
        }))
        .send()
        .await
        .unwrap_or_else(|error| panic!("update exchanged profile: {error}"))
        .json()
        .await
        .unwrap_or_else(|error| panic!("decode updated exchanged profile: {error}"));
    assert_eq!(updated["profile"]["display_name"], "Exchanged Guest");
    assert_eq!(updated["profile"]["avatar_image_url"], replacement_avatar);
    assert_eq!(updated["profile"]["mic_muted"], true);
    assert_eq!(updated["profile"]["deafened"], true);

    let participant = store
        .participant("general", "invite-boundary-guest")
        .await
        .unwrap_or_else(|error| panic!("inspect exchanged participant projection: {error}"));
    assert_eq!(participant.display_name, "Exchanged Guest");
    assert_eq!(participant.avatar_image_url, replacement_avatar);
    assert!(!participant.muted);
    let old_avatar = client
        .get(format!("{base_url}{admitted_avatar}"))
        .send()
        .await
        .unwrap_or_else(|error| panic!("read replaced admitted avatar: {error}"));
    assert_eq!(old_avatar.status(), reqwest::StatusCode::NOT_FOUND);
    assert_avatar_available(client, base_url, replacement_avatar).await;
    replacement_avatar.to_owned()
}

pub(crate) async fn issue_profile_ticket(
    client: &Client,
    base_url: &str,
    session_token: &str,
) -> String {
    let response = client
        .post(format!("{base_url}/api/session-tickets/profile"))
        .header("authorization", format!("Bearer {session_token}"))
        .header("origin", "tauri://localhost")
        .send()
        .await
        .unwrap_or_else(|error| panic!("exchange human profile ticket: {error}"));
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    assert_eq!(
        response.headers()["access-control-allow-origin"],
        "tauri://localhost"
    );
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    let response: Value = response
        .json()
        .await
        .unwrap_or_else(|error| panic!("decode human profile ticket: {error}"));
    assert!(response["ttl_seconds"].as_u64().is_some_and(|ttl| ttl > 0));
    response["ticket"]
        .as_str()
        .unwrap_or_else(|| panic!("human profile ticket is missing"))
        .to_owned()
}

pub(crate) async fn assert_avatar_available(client: &Client, base_url: &str, avatar_url: &str) {
    let response = client
        .get(format!("{base_url}{avatar_url}"))
        .send()
        .await
        .unwrap_or_else(|error| panic!("read prejoin avatar: {error}"));
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "image/png");
    assert_eq!(response.headers()["cache-control"], "private, no-store");
}
