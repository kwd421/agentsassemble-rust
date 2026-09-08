use std::time::Duration;

use agentsassemble_domain::ProviderRequestResolution;
use serde_json::json;

use super::provider_turn_tests::{active_session, requests, stop_and_release, turn_fixture};
use super::{ProviderAdapter, ProviderTurnRequest, tests::fixture_session};
use crate::{ProviderRequestExchange, ProviderRequestIngress};

#[tokio::test]
async fn codex_early_request_delivers_once_and_interrupt_cancels_live_wait()
-> Result<(), Box<dyn std::error::Error>> {
    let _serial = super::tests::RUNTIME_TEST_LOCK.lock().await;
    for interrupt in [false, true] {
        run_request(interrupt).await?;
    }
    Ok(())
}

async fn run_request(interrupt: bool) -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let transcript = directory.path().join("requests.jsonl");
    let script = request_fixture(&transcript, interrupt);
    let session = fixture_session(directory.path(), &script).await;
    let adapter = ProviderAdapter::new();
    let started = adapter.start(&session).await?;
    let active = active_session(&session, &started, "room-turn-1");
    let (ingress, mut commands) = ProviderRequestIngress::channel(1);
    let request = ProviderTurnRequest {
        request_ingress: Some(ingress),
        turn_id: "room-turn-1".to_owned(),
        turn_generation: 1,
        execution_id: "11111111-1111-4111-8111-111111111111".to_owned(),
        input: "Ask a question".to_owned(),
        room_observation: None,
    };
    let prepared = adapter.prepare_turn(&active, &request).await?;
    let authority = prepared.exact_authority();
    let turn_adapter = adapter.clone();
    let turn_session = active.clone();
    let turn_request = request.clone();
    let turn = tokio::spawn(async move {
        turn_adapter
            .send_prepared_turn(prepared, &turn_session, &turn_request)
            .await
    });
    let command = tokio::time::timeout(Duration::from_secs(5), commands.recv())
        .await?
        .ok_or("native request channel closed")?;
    assert_eq!(command.session_id, active.public.session_id);
    assert_eq!(command.turn_generation, request.turn_generation);
    assert_eq!(command.execution_id, request.execution_id);
    let (exchange, mut responder, mut delivery) = ProviderRequestExchange::channel();
    command.complete(Ok(exchange));
    if interrupt {
        let mut control = adapter.begin_exact_turn(&authority).await?;
        control.request_interrupt();
        control.wait_quiesced(Duration::from_secs(5)).await?;
        assert!(!delivery.completion().await);
        assert_eq!(
            turn.await?.err().ok_or("expected interrupted turn")?.code,
            "provider_turn_interrupted"
        );
    } else {
        responder.respond(ProviderRequestResolution::Answers {
            answers: [("answer".to_owned(), vec!["fixture-value".to_owned()])].into(),
        })?;
        assert!(delivery.completion().await);
        assert!(
            !turn.is_finished(),
            "completion must await room delivery receipt"
        );
        delivery.finish(Ok(()));
        turn.await??;
    }
    let recorded = requests(&transcript);
    assert_eq!(recorded.len(), 5);
    if interrupt {
        assert_eq!(recorded[4]["method"], "turn/interrupt");
    } else {
        assert_eq!(
            recorded[4],
            json!({"jsonrpc": "2.0", "id": "native-request-1", "result": {"answers": {"answer": {"answers": ["fixture-value"]}}}})
        );
    }
    stop_and_release(&adapter, &active, &started).await;
    Ok(())
}

pub(crate) fn request_fixture(transcript: &std::path::Path, interrupt: bool) -> String {
    let native_request = json!({"id": "native-request-1", "method": "item/tool/requestUserInput", "params": {
        "threadId": "thread-1", "turnId": "provider-turn-1", "questions": [{
            "id": "answer", "header": "Input", "question": "Enter the fixture answer", "isSecret": true, "isOther": true
        }]
    }});
    let before = format!("printf '%s\\n' '{native_request}'\n");
    let completed_status = if interrupt {
        "interrupted"
    } else {
        "completed"
    };
    let completion = json!({"method": "turn/completed", "params": {"threadId": "thread-1", "turn": {"id": "provider-turn-1", "status": completed_status}}});
    let notifications = format!(
        "IFS= read -r response\nprintf '%s\\n' \"$response\" >> '{}'\n{}printf '%s\\n' '{{\"method\":\"item/completed\",\"params\":{{\"threadId\":\"thread-1\",\"turnId\":\"provider-turn-1\",\"item\":{{\"type\":\"agentMessage\",\"text\":\"done\"}}}}}}'\nprintf '%s\\n' '{}'\n",
        transcript.display(),
        if interrupt {
            "printf '%s\\n' '{\"id\":4,\"result\":{}}'\n"
        } else {
            ""
        },
        completion,
    );
    turn_fixture(
        transcript,
        &before,
        "{\"id\":3,\"result\":{\"turn\":{\"id\":\"provider-turn-1\"}}}",
        &notifications,
    )
}
