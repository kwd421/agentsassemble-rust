use super::*;
use agentsassemble_domain::ProviderRequestOption;
use serde_json::json;

#[tokio::test]
async fn valid_large_questions_preserve_owner_reconnect_and_answer() -> TestResult {
    let (store, human, bearer) = admitted_human().await?;
    let server = human_invite::start(store.clone()).await;
    let mut manager = local_socket::connect(&server.base_url, server.state(), "general").await;
    manager.subscribe(0).await;
    let snapshot = manager.receive_json().await;
    manager.send_json(&json!({"op":"command","request_id":Uuid::new_v4(),"action":"room.settings.update","payload":{"expected_revision":snapshot["room_settings"]["settings_revision"],"conversation_mode":"ambient"}})).await;
    receive_result(&mut manager, "ack").await;
    manager.close().await;
    let mut native = Vec::new();
    let mut ids = Vec::new();
    for _ in 0..4 {
        let (connection, mut request, _) =
            assigned_request_with_handle(&store, &human, Uuid::new_v4().to_string()).await?;
        request.request.timeout_seconds = 600;
        request.request.prompt = ProviderRequestPrompt::Answers {
            questions: (0..3)
                .map(|q| ProviderRequestQuestion {
                    id: format!("q{q}"),
                    header: "Question".to_owned(),
                    question: "질".repeat(800),
                    options: (0..12)
                        .map(|o| ProviderRequestOption {
                            id: format!("option{o}"),
                            label: format!("{o}{}", "가".repeat(238)),
                            kind: "option".to_owned(),
                            description: "나".repeat(400),
                        })
                        .collect(),
                    multiple: false,
                    is_other: false,
                    is_secret: false,
                })
                .collect(),
        };
        assert!(request.request.is_valid());
        assert!(
            serde_json::to_vec(&request.request)?.len()
                < agentsassemble_protocol::MAX_ROOM_SOCKET_MESSAGE_BYTES
        );
        ids.push(request.request.provider_request_id);
        let opened = server
            .rooms()
            .open_attendee_request(connection, request)
            .await?;
        native.push(opened.exchange.ok_or("exchange missing")?);
    }
    let client = reqwest::Client::new();
    // Both initial admission and a second connection must recover all bodies.
    for _ in 0..2 {
        let mut owner = human_invite::open_session_socket(&client, &server.base_url, &bearer).await;
        assert_eq!(owner.initial_requests.len(), 4);
        assert!(
            serde_json::to_vec(&owner.initial_requests)?.len()
                > agentsassemble_protocol::MAX_ROOM_SOCKET_MESSAGE_BYTES
        );
        for id in &ids {
            let body = owner
                .initial_requests
                .iter()
                .find(|body| body["request"]["provider_request_id"] == id.to_string())
                .ok_or("pending body missing")?;
            assert_eq!(
                body["request"]["prompt"]["questions"][2]["options"][11]["description"],
                "나".repeat(400)
            );
        }
        owner.close().await;
    }
    let mut owner = human_invite::open_session_socket(&client, &server.base_url, &bearer).await;
    for (id, exchange) in ids.iter().zip(&mut native) {
        owner.send_json(&json!({"op":"command","request_id":id,"action":"provider.request.resolve","payload":{"response_kind":"answers","answers":{"q0":[format!("0{}", "가".repeat(238))],"q1":[format!("1{}", "가".repeat(238))],"q2":[format!("2{}", "가".repeat(238))]}}})).await;
        receive_result(&mut owner, "ack").await;
        assert!(matches!(
            exchange.receive().await?,
            ProviderRequestResolution::Answers { .. }
        ));
        exchange.complete(true).await?;
    }
    assert!(
        store
            .pending_provider_request_ids("general")
            .await?
            .is_empty()
    );
    owner.close().await;
    server.stop().await;
    Ok(())
}
