use super::*;
use agentsassemble_provider::ProviderCatalogService;

// One end-to-end pass for owner-confirmed agent removal on the connector entry: the
// pending request changes nothing and binds an active agent target, and the owner's
// approval runs the same participant-removal core as the owner's own agent kick,
// with delegate attribution.
//
// The target is created through the owner's own `agent.create` core so this pass
// consumes one owner confirmation, keeping the shared owner confirmation budget for
// the earlier steps of the same flow.
pub(super) async fn verify(
    client: &Client,
    store: &SqliteStore,
    delegate: &str,
    connection: Option<&Value>,
    owner: (&str, &agentsassemble_server::AppState),
) -> TestResult {
    let directory = tempfile::tempdir()?;
    let workspace = directory
        .path()
        .to_str()
        .ok_or("workspace not UTF-8")?
        .to_owned();
    let catalog = ProviderCatalogService::fixed(crate::support::provider_fixture::agent_catalog(
        directory.path(),
        None,
    ));
    let request_id = Uuid::new_v4().to_string();
    let create = json!({
        "provider_id": "codex",
        "catalog_revision": "catalog-boundary-1",
        "display_name": "Removal Target",
        "workspace": workspace,
        "model": "gpt-5.6-terra",
        "permission_mode": "meeting_read_only",
        "start_now": false,
    });
    let selection = catalog
        .validate_creation("general", LOCAL_OPERATOR_USER_ID, &request_id, &create)
        .await?;
    let created = store
        .execute_agent_create(
            RoomMutationAuthority::TrustedPrincipal(&operator()),
            &request_id,
            &create,
            &selection.into(),
        )
        .await?;
    let target = created.result["agent_session"]["session_id"]
        .as_str()
        .ok_or("missing target")?
        .to_owned();

    let mut args = json!({
        "request_id": Uuid::new_v4().to_string(),
        "agent_id": target,
    });
    if let Some(connection) = connection {
        args["connection_id"] = connection.clone();
    }
    let before = store.snapshot("general", 0, 400).await?;
    let pending = call(client, "request_remove_agent", args.clone()).await;
    assert_eq!(pending["status"], "pending_confirmation", "{pending}");
    let confirmation_id = pending["confirmation_id"]
        .as_str()
        .ok_or("missing confirmation")?
        .to_owned();
    let after = store.snapshot("general", 0, 400).await?;
    assert_eq!(before.agent_sessions, after.agent_sessions);
    assert_eq!(
        store.participant("general", &target).await?.status,
        agentsassemble_domain::ParticipantStatus::Detached
    );
    assert_eq!(
        pending,
        call(client, "request_remove_agent", args.clone()).await
    );
    // Retargeting under the same request identity conflicts instead.
    let mut changed = args.clone();
    changed["agent_id"] = json!("missing");
    let conflict = room_portal_fixture::call_tool(client, "request_remove_agent", changed).await;
    assert_eq!(conflict.is_error, Some(true));
    assert!(
        conflict.content[0]
            .as_text()
            .unwrap()
            .text
            .contains("request_conflict"),
        "{conflict:?}"
    );
    // The owner approves through the room socket, so the room actor commits the
    // removal through the same core as the owner's own agent kick.
    let ack = super::decide_on_socket(owner, &confirmation_id, "approve", None).await;
    assert_eq!(ack["op"], "ack", "{ack}");
    assert_eq!(ack["result"]["state"], "succeeded", "{ack}");
    let done = call(client, "request_remove_agent", args).await;
    assert_eq!(done["status"], "succeeded", "{done}");
    assert_eq!(done["confirmation_id"], confirmation_id);
    assert_eq!(done["target_id"], target);
    for forbidden in [
        "credential",
        "bearer",
        "token",
        "secret",
        "password",
        "workspace",
        "profile",
    ] {
        assert!(
            !serde_json::to_string(&done)?.contains(forbidden),
            "AI result carries {forbidden}: {done}"
        );
    }
    assert_eq!(
        store.participant("general", &target).await?.status,
        agentsassemble_domain::ParticipantStatus::Kicked
    );
    let snapshot = store.snapshot("general", 0, 400).await?;
    let removed = snapshot
        .agent_sessions
        .iter()
        .find(|agent| agent.session_id == target)
        .ok_or("missing removed agent")?;
    assert!(!removed.enabled);
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
    catalog.shutdown().await?;
    Ok(())
}
