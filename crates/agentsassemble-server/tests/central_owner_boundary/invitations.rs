use super::{DEVICE, ORIGIN, SECRET};
use axum::http::StatusCode;
use reqwest::{Client, RequestBuilder};
use serde_json::{Value, json};
use std::net::SocketAddr;

fn authorized(request: RequestBuilder, session: &str, device: &str) -> RequestBuilder {
    request
        .header("host", "owner.example.test")
        .header("x-forwarded-proto", "https")
        .header("x-agentsassemble-proxy-token", SECRET)
        .header("origin", ORIGIN)
        .header("x-device-token", device)
        .bearer_auth(session)
}

async fn sent(request: RequestBuilder, status: StatusCode) -> Value {
    let response = request
        .send()
        .await
        .unwrap_or_else(|error| panic!("invite transport: {error:?}"));
    let actual = response.status();
    let value: Value = response
        .json()
        .await
        .unwrap_or_else(|error| panic!("invite JSON: {error:?}"));
    assert_eq!(
        actual,
        status,
        "invite rejection: {}",
        value.get("error").unwrap_or(&Value::Null)
    );
    value
}

pub(super) async fn verify(client: &Client, address: SocketAddr, admission: &Value) {
    let session = admission["session_token"]
        .as_str()
        .unwrap_or_else(|| panic!("owner session"));
    let base = format!("http://{address}/api/central-owner");
    for path in [
        "/room-invite/create",
        "/room-invite/revoke",
        "/room-connector/invite",
        "/room-attendee/friend-invite",
        "/operator-pairing/create",
        "/operator-pairing/revoke",
    ] {
        sent(
            authorized(
                client.post(format!("{base}{path}")),
                session,
                "aad1_BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBA",
            )
            .json(&json!({})),
            if path.starts_with("/room-invite/")
                || path == "/room-connector/invite"
                || path == "/room-attendee/friend-invite"
            {
                StatusCode::FORBIDDEN
            } else {
                StatusCode::UNAUTHORIZED
            },
        )
        .await;
    }
    let origin = sent(
        authorized(
            client.get(format!("{base}/public-invite/origin")),
            session,
            DEVICE,
        ),
        StatusCode::OK,
    )
    .await;
    assert_eq!(origin, json!({"public_url": ORIGIN}));
    let human = sent(authorized(client.post(format!("{base}/room-invite/create")), session, DEVICE)
        .json(&json!({"meeting_id": "general", "display_name": "Web guest", "invite_scope": "read_only", "ttl_seconds": 3600, "max_uses": 5})), StatusCode::OK).await;
    assert_eq!(human["meeting_id"], "general");
    assert_eq!(human["invite_scope"], "read_only");
    assert_eq!(human["max_uses"], 5);
    sent(
        authorized(
            client.post(format!("{base}/room-invite/create")),
            session,
            DEVICE,
        )
        .json(&json!({"meeting_id": "second"})),
        StatusCode::UNAUTHORIZED,
    )
    .await;
    for _ in 0..2 {
        let revoked = sent(
            authorized(
                client.post(format!("{base}/room-invite/revoke")),
                session,
                DEVICE,
            )
            .json(&json!({"meeting_id": "general", "invite_id": human["invite_id"]})),
            StatusCode::OK,
        )
        .await;
        assert_eq!(revoked["status"], "revoked");
    }
    verify_connector(client, &base, session, admission).await;
    verify_friend(client, &base, session, admission).await;
    verify_pairing(client, address, &base, session, admission).await;
}

async fn verify_connector(client: &Client, base: &str, session: &str, admission: &Value) {
    let body = json!({"request_id": "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb", "scope": "read_write", "reach": "public"});
    let mut first = Value::Null;
    for replay in [false, true] {
        let invite = sent(
            authorized(
                client.post(format!("{base}/room-connector/invite")),
                session,
                DEVICE,
            )
            .json(&body),
            StatusCode::OK,
        )
        .await;
        assert_eq!(invite["room_uid"], admission["room_uid"]);
        if replay {
            assert_eq!(invite, first);
        } else {
            first = invite;
        }
    }
    let mut local = body;
    local["request_id"] = json!("cccccccc-cccc-4ccc-8ccc-cccccccccccc");
    local["reach"] = json!("local");
    sent(
        authorized(
            client.post(format!("{base}/room-connector/invite")),
            session,
            DEVICE,
        )
        .json(&local),
        StatusCode::BAD_REQUEST,
    )
    .await;
}

async fn verify_friend(client: &Client, base: &str, session: &str, admission: &Value) {
    let friend_id = "dddddddd-dddd-4ddd-8ddd-dddddddddddd";
    sent(authorized(client.post(format!("{base}/friends")), session, DEVICE)
        .json(&json!({"friend_id": friend_id, "expected_revision": 0,
            "details": {"display_name": "Web AI", "handle": "", "participant_type": "subscription_ai",
                "provider_kind": "codex", "connection_kind": "cli", "agent_id": "", "source_agent_id": "",
                "last_meeting_id": "", "status": "offline", "source": "manual", "last_seen_at": null}})), StatusCode::OK).await;
    let body =
        json!({"request_id": "eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee", "friend_id": friend_id});
    let mut first = Value::Null;
    for replay in [false, true] {
        let invite = sent(
            authorized(
                client.post(format!("{base}/room-attendee/friend-invite")),
                session,
                DEVICE,
            )
            .json(&body),
            StatusCode::OK,
        )
        .await;
        assert_eq!(invite["room_uid"], admission["room_uid"]);
        assert_eq!(invite["provider"], "codex");
        if replay {
            assert_eq!(invite, first);
        } else {
            first = invite;
        }
    }
}

async fn verify_pairing(
    client: &Client,
    address: SocketAddr,
    base: &str,
    session: &str,
    admission: &Value,
) {
    let authority = json!({"server_id": admission["server_id"], "authority_lineage_id": admission["authority_lineage_id"],
        "room_id": "general", "room_uid": admission["room_uid"]});
    let mut stale = authority.clone();
    stale["room_uid"] = json!("ffffffff-ffff-4fff-8fff-ffffffffffff");
    sent(
        authorized(
            client.post(format!("{base}/operator-pairing/create")),
            session,
            DEVICE,
        )
        .json(&stale),
        StatusCode::UNAUTHORIZED,
    )
    .await;
    let pairing = sent(
        authorized(
            client.post(format!("{base}/operator-pairing/create")),
            session,
            DEVICE,
        )
        .json(&authority),
        StatusCode::OK,
    )
    .await;
    let url = url::Url::parse(
        pairing["pairing_url"]
            .as_str()
            .unwrap_or_else(|| panic!("pairing URL")),
    )
    .unwrap_or_else(|error| panic!("pairing URL: {error:?}"));
    let token = url
        .query_pairs()
        .find(|(key, _)| key == "token")
        .unwrap_or_else(|| panic!("pairing token"))
        .1
        .into_owned();
    let redemption = sent(
        authorized(
            client.post(format!("http://{address}/api/operator-pairing/redeem")),
            "",
            DEVICE,
        )
        .json(&json!({"pairing_token": token})),
        StatusCode::OK,
    )
    .await;
    let ordinary = redemption["session_token"]
        .as_str()
        .unwrap_or_else(|| panic!("paired session"));
    for path in [
        "/room-invite/create",
        "/room-invite/revoke",
        "/room-connector/invite",
        "/room-attendee/friend-invite",
        "/operator-pairing/create",
        "/operator-pairing/revoke",
    ] {
        sent(
            authorized(client.post(format!("{base}{path}")), ordinary, DEVICE).json(&json!({})),
            StatusCode::UNAUTHORIZED,
        )
        .await;
    }
    sent(
        authorized(
            client.post(format!("{base}/operator-pairing/revoke")),
            session,
            DEVICE,
        )
        .json(&json!({"authority": authority, "pairing_id": pairing["pairing_id"]})),
        StatusCode::OK,
    )
    .await;
    sent(
        authorized(
            client.get(format!("{base}/public-invite/origin")),
            ordinary,
            DEVICE,
        ),
        StatusCode::UNAUTHORIZED,
    )
    .await;
}
