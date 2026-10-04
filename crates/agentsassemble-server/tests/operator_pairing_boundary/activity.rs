use super::*;
use agentsassemble_persistence::{
    OperatorSessionAuthorization, RoomSessionAuthorization, ServerOwnerAuthority,
};
use sha2::{Digest, Sha256};

type TestResult = Result<(), Box<dyn std::error::Error>>;

struct ActivityFixture {
    server: PairingServer,
    client: Client,
    session: String,
    device: String,
    authorization: OperatorSessionAuthorization,
    old: i64,
}

impl ActivityFixture {
    async fn new(age_seconds: i64) -> Result<Self, Box<dyn std::error::Error>> {
        let server = PairingServer::start().await;
        let store = &server.state.store;
        let manager = store
            .authorize_local_room_manager(
                "general",
                LOCAL_OPERATOR_USER_ID,
                LOCAL_OPERATOR_PARTICIPANT_ID,
            )
            .await?;
        let now = chrono::Utc::now() - chrono::Duration::seconds(age_seconds);
        let device = format!("aad1_{}", URL_SAFE_NO_PAD.encode([0x91; 32]));
        let fingerprint: [u8; 32] = Sha256::digest(device.as_bytes()).into();
        store
            .create_operator_pairing(
                &agentsassemble_persistence::RoomManagerAuthority::Local(manager),
                &[91; 32],
                ORIGIN,
                now,
            )
            .await?;
        let paired = store
            .redeem_operator_pairing(&[91; 32], &fingerprint, ORIGIN, now)
            .await?;
        Ok(Self {
            server,
            client: Client::new(),
            session: paired.session_bearer,
            device,
            authorization: paired.authorization,
            old: now.timestamp(),
        })
    }

    fn request(&self, method: reqwest::Method, path: &str) -> RequestBuilder {
        public(
            self.client
                .request(method, format!("{}{path}", self.server.base)),
        )
        .bearer_auth(&self.session)
        .header("x-device-token", &self.device)
    }

    async fn last_use(&self) -> i64 {
        self.server
            .state
            .store
            .owner_device_sessions(&ServerOwnerAuthority::LocalOperator)
            .await
            .unwrap_or_else(|error| panic!("{error}"))[0]
            .last_connected_at
            .unwrap_or_else(|| panic!("activity missing"))
    }

    async fn socket_ticket(&self) -> String {
        // Arrange an already issued ticket without HTTP exchange changing the timestamp.
        self.server
            .state
            .tickets
            .issue_room_session_socket(RoomSessionAuthorization::Operator(
                self.authorization.clone(),
            ))
            .await
            .unwrap_or_else(|error| panic!("{error}"))
            .ticket
    }

    async fn close(self) {
        self.server.shutdown.cancel();
        self.server
            .running
            .await
            .unwrap_or_else(|error| panic!("{error}"))
            .unwrap_or_else(|error| panic!("{error}"));
    }
}

#[tokio::test]
async fn rejected_http_does_not_refresh_device_activity() -> TestResult {
    let fixture = ActivityFixture::new(120).await?;
    let mut refreshed = Vec::new();
    for (method, path, body, expected) in [
        (
            reqwest::Method::GET,
            "/api/user-profile",
            None,
            StatusCode::UNAUTHORIZED,
        ),
        (
            reqwest::Method::POST,
            "/api/session-tickets/socket",
            Some(json!({"unexpected": true})),
            StatusCode::BAD_REQUEST,
        ),
        (
            reqwest::Method::GET,
            "/api/central-owner/friends",
            None,
            StatusCode::UNAUTHORIZED,
        ),
        (
            reqwest::Method::GET,
            "/api/room-pins?room_id=foreign&channel_id=lobby",
            None,
            StatusCode::UNAUTHORIZED,
        ),
    ] {
        let request = fixture.request(method, path);
        let response = match body {
            Some(body) => request.json(&body),
            None => request,
        }
        .send()
        .await?;
        assert_eq!(response.status(), expected, "{path}");
        if fixture.last_use().await != fixture.old {
            refreshed.push(path);
        }
    }
    fixture.close().await;
    assert!(
        refreshed.is_empty(),
        "rejected requests refreshed activity: {refreshed:?}"
    );
    Ok(())
}

#[tokio::test]
async fn successful_http_refreshes_device_activity_and_coalesces_writes() -> TestResult {
    for path in [
        "/api/room-settings?room_id=general",
        "/api/room-pins?room_id=general&channel_id=lobby",
        "/api/room-search?room_id=general&channel_id=lobby&q=paired",
    ] {
        let fixture = ActivityFixture::new(120).await?;
        assert_eq!(
            fixture
                .request(reqwest::Method::GET, path)
                .send()
                .await?
                .status(),
            StatusCode::OK
        );
        assert!(fixture.last_use().await > fixture.old, "{path}");
        fixture.close().await;
    }
    let fixture = ActivityFixture::new(120).await?;
    let response = fixture.request(reqwest::Method::POST, "/api/room-settings")
        .json(&json!({"room_id":"general", "room_uid":fixture.server.authority["room_uid"], "appearance":{"notifications":"mute"}}))
        .send().await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(fixture.last_use().await > fixture.old);
    fixture.close().await;
    let fixture = ActivityFixture::new(10).await?;
    assert_eq!(
        fixture
            .request(reqwest::Method::GET, "/api/room-settings?room_id=general")
            .send()
            .await?
            .status(),
        StatusCode::OK
    );
    assert_eq!(fixture.last_use().await, fixture.old);
    fixture.close().await;
    Ok(())
}

#[tokio::test]
async fn socket_ticket_capacity_failure_does_not_refresh_device_activity() -> TestResult {
    let fixture = ActivityFixture::new(120).await?;
    assert_eq!(
        fixture
            .request(reqwest::Method::POST, "/api/session-tickets/socket")
            .send()
            .await?
            .status(),
        StatusCode::OK
    );
    assert!(fixture.last_use().await > fixture.old);
    fixture.close().await;
    let fixture = ActivityFixture::new(120).await?;
    for _ in 0..8 {
        fixture.socket_ticket().await;
    }
    assert_eq!(
        fixture
            .request(reqwest::Method::POST, "/api/session-tickets/socket")
            .send()
            .await?
            .status(),
        StatusCode::TOO_MANY_REQUESTS
    );
    assert_eq!(fixture.last_use().await, fixture.old);
    fixture.close().await;
    Ok(())
}

#[tokio::test]
async fn socket_nacks_do_not_refresh_but_authorized_frames_do() -> TestResult {
    for accepted in [false, true] {
        let fixture = ActivityFixture::new(120).await?;
        let ticket = fixture.socket_ticket().await;
        let (wire, _) = tokio_tungstenite::connect_async(format!(
            "ws://{}/ws?ticket={ticket}",
            fixture.server.address
        ))
        .await?;
        let mut socket = RoomSocketPeer::new(wire);
        if accepted {
            assert_eq!(socket.subscribe(0).await["op"], "subscribed");
            assert_eq!(socket.receive_json().await["op"], "snapshot");
            // Ordering barrier: subscription bookkeeping finishes before this NACK.
            socket
                .send_json(
                    &json!({"op":"subscribe", "streams":["room_events"], "resume_from_seq":0}),
                )
                .await;
            assert_eq!(socket.receive_json().await["op"], "nack");
            assert!(fixture.last_use().await > fixture.old);
            socket.close().await;
        } else {
            socket
                .send_json(&json!({"op":"ping", "nonce":"not-subscribed"}))
                .await;
            assert_eq!(socket.receive_json().await["op"], "nack");
            assert!(socket.wait_closed().await);
            assert_eq!(fixture.last_use().await, fixture.old);
        }
        fixture.close().await;
    }
    Ok(())
}

#[tokio::test]
async fn denied_posting_permission_does_not_refresh_device_activity() -> TestResult {
    let fixture = ActivityFixture::new(120).await?;
    fixture
        .server
        .state
        .store
        .execute_participant_mute(
            agentsassemble_persistence::RoomMutationAuthority::TrustedPrincipal(
                fixture.authorization.principal(),
            ),
            "native-mute",
            &json!({"participant_id":LOCAL_OPERATOR_PARTICIPANT_ID,"muted":true}),
        )
        .await?;
    let response = fixture
        .request(reqwest::Method::POST, "/api/message-attachments")
        .json(&json!({"filename":"denied.txt", "content_type":"text/plain", "data_base64":"bm8="}))
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(response.json::<Value>().await?["code"], "muted");
    assert_eq!(fixture.last_use().await, fixture.old);
    let response = fixture.request(reqwest::Method::POST, "/api/room-attendee/companion-invite")
        .json(&json!({"request_id":uuid::Uuid::new_v4(),"provider":"codex","display_name":"Denied companion"})).send().await?;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        response.json::<Value>().await?["error"]["code"],
        "permission_denied"
    );
    assert_eq!(fixture.last_use().await, fixture.old);
    fixture.close().await;
    Ok(())
}
