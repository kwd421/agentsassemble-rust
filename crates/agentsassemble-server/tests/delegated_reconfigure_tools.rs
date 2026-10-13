use super::*;

// One end-to-end pass for owner-confirmed agent reconfiguration on the connector
// entry: the pending request changes nothing and binds the target, and the approved
// drive re-resolves the catalog and runs the same stopped-configuration core as the
// owner's own `agent.configure`, with delegate attribution.
pub(super) async fn verify(
    client: &Client,
    store: &SqliteStore,
    delegate: &str,
    connection: Option<&Value>,
    owner: (&str, &agentsassemble_server::AppState),
) -> TestResult {
    let workspace = tempfile::tempdir()?;
    let workspace = workspace
        .path()
        .to_str()
        .ok_or("workspace not UTF-8")?
        .to_owned();
    // Create one stopped target through the delegated creation flow so the test owns
    // its identity without building a new fixture.
    let mut create = json!({
        "request_id": Uuid::new_v4().to_string(),
        "provider_id": "codex",
        "catalog_revision": "catalog-boundary-1",
        "display_name": "Reconfigure Target",
        "workspace": workspace,
        "model": "gpt-5.6-terra",
        "permission_mode": "meeting_read_only",
    });
    if let Some(connection) = connection {
        create["connection_id"] = connection.clone();
    }
    let pending = call(client, "request_create_agent", create.clone()).await;
    assert_eq!(pending["status"], "pending_confirmation", "{pending}");
    let creation = pending["confirmation_id"]
        .as_str()
        .ok_or("missing creation confirmation")?
        .to_owned();
    let ack = super::decide_on_socket(owner, &creation, "approve", None).await;
    assert_eq!(ack["op"], "ack", "{ack}");
    let created = call(client, "request_create_agent", create).await;
    assert_eq!(created["status"], "succeeded", "{created}");
    let target = created["target_id"]
        .as_str()
        .ok_or("missing target")?
        .to_owned();

    let mut args = json!({
        "request_id": Uuid::new_v4().to_string(),
        "agent_id": target,
        "catalog_revision": "catalog-boundary-1",
        "permission_mode": "workspace_write",
    });
    if let Some(connection) = connection {
        args["connection_id"] = connection.clone();
    }
    let before = store.snapshot("general", 0, 300).await?;
    let pending = call(client, "request_reconfigure_agent", args.clone()).await;
    assert_eq!(pending["status"], "pending_confirmation", "{pending}");
    let confirmation_id = pending["confirmation_id"]
        .as_str()
        .ok_or("missing confirmation")?
        .to_owned();
    let after = store.snapshot("general", 0, 300).await?;
    assert_eq!(before.agent_sessions, after.agent_sessions);
    assert!(after.last_seq > before.last_seq);
    assert_eq!(
        pending,
        call(client, "request_reconfigure_agent", args.clone()).await
    );
    // A changed change-field under the same request identity conflicts instead.
    let mut changed = args.clone();
    changed["permission_mode"] = json!("full_access");
    let conflict =
        room_portal_fixture::call_tool(client, "request_reconfigure_agent", changed).await;
    assert_eq!(conflict.is_error, Some(true));
    assert!(
        conflict.content[0]
            .as_text()
            .unwrap()
            .text
            .contains("request_conflict"),
        "{conflict:?}"
    );
    // The owner approves through the room socket, so the room actor drives the
    // reconfiguration through the same core as the owner's own `agent.configure`.
    let ack = super::decide_on_socket(owner, &confirmation_id, "approve", None).await;
    assert_eq!(ack["op"], "ack", "{ack}");
    assert_eq!(ack["result"]["state"], "executing", "{ack}");
    let done = call(client, "request_reconfigure_agent", args).await;
    assert_eq!(done["status"], "succeeded", "{done}");
    assert_eq!(done["confirmation_id"], confirmation_id);
    assert_eq!(done["target_id"], target);
    for forbidden in [
        "workspace",
        "executable",
        "params",
        "draft",
        "credential",
        "bearer",
        "secret",
        "password",
    ] {
        assert!(
            !serde_json::to_string(&done)?.contains(forbidden),
            "AI result carries {forbidden}: {done}"
        );
    }
    let snapshot = store.snapshot("general", 0, 300).await?;
    let agent = snapshot
        .agent_sessions
        .iter()
        .find(|agent| agent.session_id == target)
        .ok_or("missing reconfigured agent")?;
    assert_eq!(agent.permission_mode, "workspace_write");
    assert_eq!(
        agent.runtime_status,
        agentsassemble_domain::AgentRuntimeStatus::Stopped
    );
    assert!(!agent.enabled);
    let attributed: Vec<_> = snapshot
        .events
        .iter()
        .filter(|event| event.extra.get("confirmation_id") == Some(&json!(confirmation_id)))
        .collect();
    assert!(attributed.len() >= 2);
    for event in &attributed {
        assert_eq!(event.actor.participant_id, delegate);
        assert_eq!(
            event.extra["on_behalf_of_participant_id"],
            LOCAL_OPERATOR_PARTICIPANT_ID
        );
    }
    assert!(
        attributed
            .iter()
            .any(|event| event.message_kind.as_deref() == Some("system")
                && event
                    .content
                    .as_ref()
                    .is_some_and(|text| text.contains("on behalf of"))),
        "{done}"
    );
    Ok(())
}
