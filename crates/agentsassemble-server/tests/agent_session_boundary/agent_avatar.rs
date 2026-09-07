use super::*;

pub(super) async fn assert_avatar_flow<S>(
    socket: &mut RoomSocketPeer<S>,
    server: &RunningServer,
    session_id: &str,
) where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let request = avatar_manager_request(server).await;
    let client = Client::new();
    // This canonical 1x1 PNG is the same bounded raster accepted by the profile HTTP owner.
    let payload = json!({"filename": "agent.png", "content_type": "image/png",
        "data_base64": "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGMQ0bD5DwACRAF4aig0hQAAAABJRU5ErkJggg=="});
    assert_rejected_upload_tickets(&client, server, &request, session_id, &payload).await;
    let upload_url = format!("{}/api/agent-avatars/upload/{session_id}", server.base_url);
    let ticket = upload_ticket(server, &request, session_id).await;
    let response = client
        .post(&upload_url)
        .bearer_auth(&ticket)
        .json(&payload)
        .send()
        .await
        .unwrap_or_else(|error| panic!("avatar upload: {error}"));
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let uploaded: Value = response
        .json()
        .await
        .unwrap_or_else(|error| panic!("upload JSON: {error}"));
    let reference = uploaded["attachment"]["url"]
        .as_str()
        .unwrap_or_else(|| panic!("avatar reference"));
    let read_url = format!("{}{reference}", server.base_url);
    assert_eq!(
        client
            .get(&read_url)
            .send()
            .await
            .unwrap_or_else(|error| panic!("pending read: {error}"))
            .status(),
        reqwest::StatusCode::NOT_FOUND
    );
    send_command(
        socket,
        "avatar-bind",
        "agent.profile.update",
        &json!({"agent_id": session_id, "avatar_image_url": reference}),
    )
    .await;
    let bound = receive_until_ack(socket, 3).await;
    assert_eq!(
        bound["result"]["agent_session"]["avatar_image_url"],
        reference
    );
    assert_eq!(
        bound["result"]["participant"]["avatar_image_url"],
        reference
    );
    assert_eq!(bound["result"]["events"][0]["avatar_image_url"], reference);
    let image = client
        .get(&read_url)
        .send()
        .await
        .unwrap_or_else(|error| panic!("current read: {error}"));
    assert_eq!(image.status(), reqwest::StatusCode::OK);
    assert_eq!(image.headers()["content-type"], "image/png");
    assert!(
        image.headers()["cache-control"]
            .to_str()
            .unwrap_or("")
            .contains("no-store")
    );
    assert!(
        !image
            .bytes()
            .await
            .unwrap_or_else(|error| panic!("current bytes: {error}"))
            .is_empty()
    );
    send_command(
        socket,
        "avatar-clear",
        "agent.profile.update",
        &json!({"agent_id": session_id, "avatar_image_url": ""}),
    )
    .await;
    let cleared = receive_until_ack(socket, 3).await;
    assert_eq!(cleared["result"]["agent_session"]["avatar_image_url"], "");
    assert_eq!(
        client
            .get(&read_url)
            .send()
            .await
            .unwrap_or_else(|error| panic!("cleared read: {error}"))
            .status(),
        reqwest::StatusCode::NOT_FOUND
    );
}

async fn upload_ticket(
    server: &RunningServer,
    request: &agentsassemble_server::ManagerRoomAuthorityRequest,
    session_id: &str,
) -> String {
    agentsassemble_server::issue_agent_avatar_upload_ticket(&server.state, request, session_id)
        .await
        .unwrap_or_else(|error| panic!("avatar upload ticket: {error}"))
        .ticket
}

async fn assert_rejected_upload_tickets(
    client: &Client,
    server: &RunningServer,
    request: &agentsassemble_server::ManagerRoomAuthorityRequest,
    session_id: &str,
    payload: &Value,
) {
    let ticket = upload_ticket(server, request, session_id).await;
    let wrong_target = client
        .post(format!(
            "{}/api/agent-avatars/upload/foreign",
            server.base_url
        ))
        .bearer_auth(&ticket)
        .json(payload)
        .send()
        .await
        .unwrap_or_else(|error| panic!("wrong target upload: {error}"));
    assert_eq!(wrong_target.status(), reqwest::StatusCode::UNAUTHORIZED);
    let upload_url = format!("{}/api/agent-avatars/upload/{session_id}", server.base_url);
    let replay = client
        .post(&upload_url)
        .bearer_auth(&ticket)
        .json(payload)
        .send()
        .await
        .unwrap_or_else(|error| panic!("consumed ticket upload: {error}"));
    assert_eq!(replay.status(), reqwest::StatusCode::UNAUTHORIZED);
    let profile_ticket = server
        .state
        .tickets
        .issue_server_operator(LOCAL_OPERATOR_USER_ID.to_owned())
        .await
        .unwrap_or_else(|error| panic!("profile ticket: {error}"));
    let wrong_purpose = client
        .post(&upload_url)
        .bearer_auth(profile_ticket.ticket)
        .json(payload)
        .send()
        .await
        .unwrap_or_else(|error| panic!("wrong purpose upload: {error}"));
    assert_eq!(wrong_purpose.status(), reqwest::StatusCode::UNAUTHORIZED);
}

async fn avatar_manager_request(
    server: &RunningServer,
) -> agentsassemble_server::ManagerRoomAuthorityRequest {
    let authority = server
        .state
        .store
        .authorize_local_room_manager(
            "general",
            LOCAL_OPERATOR_USER_ID,
            LOCAL_OPERATOR_PARTICIPANT_ID,
        )
        .await
        .unwrap_or_else(|error| panic!("avatar manager: {error}"));
    agentsassemble_server::ManagerRoomAuthorityRequest {
        server_id: authority.server_id.clone(),
        authority_lineage_id: authority.authority_lineage_id.clone(),
        room_id: "general".into(),
        room_uid: authority.room_uid.to_string(),
    }
}
