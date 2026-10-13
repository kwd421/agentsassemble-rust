use super::*;
use agentsassemble_domain::ProviderControlOption;
use agentsassemble_provider::ProviderCatalogService;

// Every caller runs the same real MCP -> transport -> room actor -> provider lifecycle flow.
pub(super) async fn verify(
    client: &Client,
    store: &SqliteStore,
    delegate: &str,
    connection: Option<&Value>,
    events: &mut tokio::sync::broadcast::Receiver<agentsassemble_domain::RoomEvent>,
    owner: (&str, &agentsassemble_server::AppState),
) -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut catalog = crate::support::provider_fixture::agent_catalog(directory.path(), None);
    let permissions = catalog.providers[0]
        .controls
        .iter_mut()
        .find(|control| control.key == "permission_mode")
        .ok_or("permission control missing")?;
    for mode in ["workspace_write", "full_access"] {
        permissions.options.push(ProviderControlOption {
            value: mode.into(),
            label: mode.into(),
            metadata: Default::default(),
        });
    }
    let catalog = ProviderCatalogService::fixed(catalog);
    let args = |agent: &str| {
        let mut value = json!({"agent_id":agent,"request_id":Uuid::new_v4()});
        if let Some(connection) = connection {
            value["connection_id"] = connection.clone();
        }
        value
    };
    for mode in ["meeting_read_only", "workspace_write", "full_access"] {
        let request_id = Uuid::new_v4().to_string();
        // Only the authenticated human creation path supplies full-access acknowledgement.
        let payload = json!({"provider_id":"codex","catalog_revision":"catalog-boundary-1",
            "display_name":format!("Lifecycle {mode}"),"workspace":directory.path(),"model":"gpt-5.6-terra",
            "permission_mode":mode,"full_access_acknowledged":mode=="full_access","start_now":false});
        let selection = catalog
            .validate_creation("general", LOCAL_OPERATOR_USER_ID, &request_id, &payload)
            .await?;
        let created = store
            .execute_agent_create(
                RoomMutationAuthority::TrustedPrincipal(&operator()),
                &request_id,
                &payload,
                &selection.into(),
            )
            .await?;
        let id = created.result["agent_session"]["session_id"]
            .as_str()
            .ok_or("missing target")?;
        let request = args(id);
        if mode != "meeting_read_only" {
            let before = store.snapshot("general", 0, 200).await?;
            let pending = call(client, "start_agent", request.clone()).await;
            assert_eq!(pending["status"], "pending_confirmation");
            let confirmation_id = pending["confirmation_id"]
                .as_str()
                .ok_or("missing confirmation")?;
            assert_eq!(pending, call(client, "start_agent", request.clone()).await);
            let after = store.snapshot("general", 0, 200).await?;
            assert_eq!(before.agent_sessions, after.agent_sessions);
            assert!(after.last_seq > before.last_seq);
            let mut changed = request.clone();
            changed["agent_id"] = json!("other-agent");
            let conflict = room_portal_fixture::call_tool(client, "start_agent", changed).await;
            assert_eq!(conflict.is_error, Some(true));
            assert!(
                conflict.content[0]
                    .as_text()
                    .unwrap()
                    .text
                    .contains("request_conflict"),
                "{conflict:?}"
            );
            if mode == "full_access" {
                let refused =
                    super::decide_on_socket(owner, confirmation_id, "approve", None).await;
                assert_eq!(refused["op"], "nack", "{refused}");
                assert_eq!(
                    refused["error"]["code"], "full_access_acknowledgement_required",
                    "{refused}"
                );
            }
            // The owner approves through the room socket, so the room actor drives the
            // reserved start through the same gate as the owner's own start.
            let ack = super::decide_on_socket(
                owner,
                confirmation_id,
                "approve",
                Some(mode == "full_access"),
            )
            .await;
            assert_eq!(ack["op"], "ack", "{ack}");
            let receipt = &ack["result"];
            assert_eq!(receipt["state"], "executing");
            // The room actor drives the approved start at once; the retry observes the
            // recorded in-progress receipt or the already completed success.
            let retried = call(client, "start_agent", request.clone()).await;
            assert!(
                retried == receipt["receipt"] || retried["status"] == "succeeded",
                "{retried} vs {receipt}"
            );
            let started = complete(client, "start_agent", request.clone(), events).await?;
            assert_eq!(started["status"], "succeeded");
            assert_eq!(started["confirmation_id"], confirmation_id);
            assert_eq!(
                store
                    .snapshot("general", 0, 200)
                    .await?
                    .agent_sessions
                    .iter()
                    .find(|agent| agent.session_id == id)
                    .ok_or("missing started agent")?
                    .runtime_status,
                agentsassemble_domain::AgentRuntimeStatus::Idle
            );
            for event in started["events"].as_array().unwrap() {
                assert_eq!(event["actor"]["participant_id"], delegate);
                assert_eq!(event["actor"]["participant_type"], "agent");
                assert_eq!(event["actor_id"], delegate);
                assert_eq!(event["actor_type"], "agent");
                assert_eq!(
                    event["on_behalf_of_participant_id"],
                    LOCAL_OPERATOR_PARTICIPANT_ID
                );
                assert_eq!(event["confirmation_id"], confirmation_id);
            }
            continue;
        }
        let started = complete(client, "start_agent", request.clone(), events).await?;
        assert_eq!(started["status"], "succeeded");
        assert_eq!(started, call(client, "start_agent", request.clone()).await);
        assert_eq!(
            store
                .snapshot("general", 0, 200)
                .await?
                .agent_sessions
                .iter()
                .find(|agent| agent.session_id == id)
                .ok_or("missing started agent")?
                .runtime_status,
            agentsassemble_domain::AgentRuntimeStatus::Idle
        );
        let mut conflict = request;
        conflict["agent_id"] = json!("other-agent");
        let rejected = room_portal_fixture::call_tool(client, "start_agent", conflict).await;
        assert_eq!(rejected.is_error, Some(true));
        assert!(
            rejected.content[0]
                .as_text()
                .unwrap()
                .text
                .contains("command_conflict"),
            "{rejected:?}"
        );
        let stop = args(id);
        let stopped = complete(client, "stop_agent", stop.clone(), events).await?;
        assert_eq!(stopped["status"], "succeeded");
        assert_eq!(stopped, call(client, "stop_agent", stop).await);
        assert_eq!(
            store
                .snapshot("general", 0, 200)
                .await?
                .agent_sessions
                .iter()
                .find(|agent| agent.session_id == id)
                .ok_or("missing stopped agent")?
                .runtime_status,
            agentsassemble_domain::AgentRuntimeStatus::Stopped
        );
        for result in [started, stopped] {
            for event in result["events"].as_array().unwrap() {
                assert_eq!(event["actor"]["participant_id"], delegate);
                assert_eq!(event["actor"]["participant_type"], "agent");
                assert_eq!(event["actor_id"], delegate);
                assert_eq!(event["actor_type"], "agent");
                assert_eq!(
                    event["on_behalf_of_participant_id"],
                    LOCAL_OPERATOR_PARTICIPANT_ID
                );
            }
            assert_eq!(result["event"]["actor_participant_id"], delegate);
            assert_eq!(
                result["event"]["on_behalf_of_participant_id"],
                LOCAL_OPERATOR_PARTICIPANT_ID
            );
            let events = store.snapshot("general", 0, 200).await?.events;
            assert_eq!(
                events
                    .iter()
                    .filter(|event| event.message_kind.as_deref() == Some("system")
                        && event.extra.get("request_id") == Some(&result["request_id"]))
                    .count(),
                1
            );
        }
    }
    catalog.shutdown().await?;
    Ok(())
}

async fn complete(
    client: &Client,
    action: &'static str,
    request: Value,
    events: &mut tokio::sync::broadcast::Receiver<agentsassemble_domain::RoomEvent>,
) -> Result<Value, Box<dyn std::error::Error>> {
    let result = call(client, action, request.clone()).await;
    if result["status"] == "succeeded" {
        return Ok(result);
    }
    assert_eq!(result["status"], "in_progress");
    assert!(
        result["operation_id"]
            .as_str()
            .is_some_and(|id| !id.is_empty())
    );
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let event = events.recv().await?;
            if event.message_kind.as_deref() == Some("system")
                && event.extra.get("request_id") == Some(&request["request_id"])
            {
                return Ok::<_, tokio::sync::broadcast::error::RecvError>(());
            }
        }
    })
    .await??;
    Ok(call(client, action, request).await)
}
