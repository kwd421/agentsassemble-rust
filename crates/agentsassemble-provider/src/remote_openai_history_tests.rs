use super::*;
use crate::driver::ProviderRoomViewDelta;

/// Two room turns on one driver; the second sends the room messages since
/// `delta_since` beside its full window. Returns every API request body.
async fn two_room_turns(delta_since: i64) -> Vec<Value> {
    let model = "deepseek-chat";
    let (endpoint, mut captured, server) = api_fixture(vec![
        tool_stream(model, "read_discussion", "tool_calls"),
        tool_stream(model, "publish_message", "tool_calls"),
        tool_stream(model, "read_discussion", "tool_calls"),
        tool_stream(model, "publish_message", "tool_calls"),
    ])
    .await;
    let credentials = ProviderCredentialStore::isolated_test_store();
    credentials
        .set(
            DEEPSEEK_SPEC.credential_id().unwrap(),
            "isolated-test-secret",
        )
        .await
        .unwrap();
    let session = workspace_session(&DEEPSEEK_SPEC, model, None).await;
    let api = RemoteOpenAiApi {
        spec: &DEEPSEEK_SPEC,
        client: reqwest::Client::builder().no_proxy().build().unwrap(),
        endpoint,
        session_endpoint_authority: None,
    };
    let mut driver = RemoteOpenAiDriver::launch_with_api(&DEEPSEEK_SPEC, credentials, api)
        .await
        .unwrap_or_else(|error| panic!("launch: {}", error.error));
    driver.attach_session(&session).await.unwrap();
    let first = room_request(&session.public.session_id);
    let mut second = first.clone();
    second.turn_id = "turn-2".to_owned();
    second.turn_generation = 2;
    second.execution_id = "00000000-0000-4000-8000-000000000098".to_owned();
    let observation = second.room_observation.as_mut().unwrap();
    observation.input_up_to_seq = 3;
    observation.view = "#1 Human: reply once\n#3 Human: and again".to_owned();
    observation.view_delta = Some(ProviderRoomViewDelta {
        since_seq: delta_since,
        view: "#3 Human: and again".to_owned(),
    });
    for request in [&first, &second] {
        driver.begin_room_observation(request).await.unwrap();
        driver.send_turn(&session, request).await.unwrap();
        driver.finish_room_observation(request).await.unwrap();
    }
    driver.stop().await.unwrap();
    server.abort();
    let _ = server.await;
    let mut bodies = Vec::new();
    while let Ok(body) = captured.try_recv() {
        bodies.push(body);
    }
    assert_eq!(bodies.len(), 4);
    bodies
}

fn read_result(body: &Value) -> &str {
    body["messages"]
        .as_array()
        .unwrap()
        .iter()
        .rev()
        .find(|message| message["role"] == "tool" && message["name"] == "read_discussion")
        .and_then(|message| message["content"].as_str())
        .unwrap()
}

#[tokio::test]
async fn the_next_turn_extends_the_last_request_and_reads_only_new_messages() {
    let bodies = two_room_turns(1).await;
    let earlier = bodies[1]["messages"].as_array().unwrap();
    let next = bodies[2]["messages"].as_array().unwrap();
    // The whole first turn is the second turn's unchanged prefix, so the
    // provider serves it from its prompt cache.
    assert!(next.len() > earlier.len());
    assert_eq!(&next[..earlier.len()], earlier.as_slice());
    assert_eq!(read_result(&bodies[3]), "#3 Human: and again");
}

#[tokio::test]
async fn a_turn_that_does_not_follow_the_held_one_opens_fresh() {
    let bodies = two_room_turns(2).await;
    assert_eq!(
        bodies[2]["messages"].as_array().unwrap().len(),
        bodies[0]["messages"].as_array().unwrap().len()
    );
    assert_eq!(
        read_result(&bodies[3]),
        "#1 Human: reply once\n#3 Human: and again"
    );
}
