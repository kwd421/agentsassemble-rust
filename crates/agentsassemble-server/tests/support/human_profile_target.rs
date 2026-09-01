use agentsassemble_persistence::SqliteStore;
use reqwest::Client;
use serde_json::{Value, json};

pub(crate) async fn assert_profile_target_boundary(
    client: &Client,
    base_url: &str,
    store: &SqliteStore,
    session_token: &str,
    admitted_avatar: &str,
) -> String {
    let retired_exchange = client
        .post(format!("{base_url}/api/session-tickets/profile"))
        .header("authorization", format!("Bearer {session_token}"))
        .send()
        .await
        .unwrap_or_else(|error| panic!("probe retired profile exchange: {error}"));
    assert_eq!(retired_exchange.status(), reqwest::StatusCode::NOT_FOUND);

    let malformed_session = client
        .get(format!("{base_url}/api/user-profile"))
        .bearer_auth("aas1.malformed")
        .send()
        .await
        .unwrap_or_else(|error| panic!("reject malformed direct session: {error}"));
    assert_eq!(
        malformed_session.status(),
        reqwest::StatusCode::UNAUTHORIZED
    );

    let profile = client
        .get(format!("{base_url}/api/user-profile"))
        .header("authorization", format!("Bearer {session_token}"))
        .send()
        .await
        .unwrap_or_else(|error| panic!("read direct-session profile: {error}"));
    assert_eq!(profile.headers()["cache-control"], "private, no-store");
    let profile: Value = profile
        .json()
        .await
        .unwrap_or_else(|error| panic!("decode direct-session profile: {error}"));
    assert_eq!(profile["profile"]["display_name"], "Boundary Guest");
    assert_eq!(profile["profile"]["avatar_image_url"], admitted_avatar);
    let upload: Value = client
        .post(format!("{base_url}/api/attachments"))
        .header("authorization", format!("Bearer {session_token}"))
        .json(&json!({
            "purpose": "profile_avatar",
            "filename": "replacement.png",
            "content_type": "image/png",
            "data_base64": "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGMQ0bD5DwACRAF4aig0hQAAAABJRU5ErkJggg=="
        }))
        .send()
        .await
        .unwrap_or_else(|error| panic!("upload direct-session profile avatar: {error}"))
        .json()
        .await
        .unwrap_or_else(|error| panic!("decode direct-session profile avatar: {error}"));
    let replacement_avatar = upload["attachment"]["url"]
        .as_str()
        .unwrap_or_else(|| panic!("direct-session profile avatar URL is missing"));

    let updated: Value = client
        .post(format!("{base_url}/api/user-profile"))
        .header("authorization", format!("Bearer {session_token}"))
        .json(&json!({
            "expected_revision": 1,
            "display_name": "Exchanged Guest",
            "avatar_image_url": replacement_avatar,
            "mic_muted": true,
            "deafened": true
        }))
        .send()
        .await
        .unwrap_or_else(|error| panic!("update direct-session profile: {error}"))
        .json()
        .await
        .unwrap_or_else(|error| panic!("decode updated direct-session profile: {error}"));
    assert_eq!(updated["profile"]["display_name"], "Exchanged Guest");
    assert_eq!(updated["profile"]["avatar_image_url"], replacement_avatar);
    assert_eq!(updated["profile"]["mic_muted"], true);
    assert_eq!(updated["profile"]["deafened"], true);

    let participant = store
        .participant("general", "invite-boundary-guest")
        .await
        .unwrap_or_else(|error| panic!("inspect direct-session participant projection: {error}"));
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
