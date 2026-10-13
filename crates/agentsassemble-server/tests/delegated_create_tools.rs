use super::*;

// Every caller runs the same tool -> room actor -> owner approval -> catalog-selected
// creation flow: the pending request changes nothing, and the approved drive uses the
// owner's own stopped-creation core with delegate attribution.
//
// The shared portal/attendee turn carries a 32-result budget that the existing flow
// nearly fills, so those surfaces prove one mode here (two paid calls); the connector
// surface owns a fresh turn and proves both modes including the full-access
// acknowledgement. Payload binding, exact retries and the acknowledgement matrix share
// the persistence path proven by unit tests.
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
    let modes = if connection.is_some() {
        vec!["meeting_read_only", "full_access"]
    } else {
        vec!["meeting_read_only"]
    };
    for mode in modes {
        let request_id = Uuid::new_v4().to_string();
        let mut args = json!({
            "request_id": request_id,
            "provider_id": "codex",
            "catalog_revision": "catalog-boundary-1",
            "display_name": format!("Delegated {mode}"),
            "workspace": workspace,
            "model": "gpt-5.6-terra",
            "permission_mode": mode,
        });
        if let Some(connection) = connection {
            args["connection_id"] = connection.clone();
        }
        let before = store.snapshot("general", 0, 300).await?;
        let pending = call(client, "request_create_agent", args.clone()).await;
        assert_eq!(pending["status"], "pending_confirmation", "{pending}");
        let confirmation_id = pending["confirmation_id"]
            .as_str()
            .ok_or("missing confirmation")?
            .to_owned();
        let after = store.snapshot("general", 0, 300).await?;
        assert_eq!(before.agent_sessions, after.agent_sessions);
        assert!(after.last_seq > before.last_seq);
        // Changed parameters under the same identity conflict; exact retries and
        // payload binding share the persistence path proven by unit tests, and the
        // shared turn budget stays for one paid call per step here.
        let mut changed = args.clone();
        changed["display_name"] = json!("Someone else");
        let conflict =
            room_portal_fixture::call_tool(client, "request_create_agent", changed).await;
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
            let refused = super::decide_on_socket(owner, &confirmation_id, "approve", None).await;
            assert_eq!(refused["op"], "nack", "{refused}");
            assert_eq!(
                refused["error"]["code"], "full_access_acknowledgement_required",
                "{refused}"
            );
        }
        // The owner approves through the room socket, so the room actor drives the
        // approved creation through the same core as the owner's own stopped create.
        let ack = super::decide_on_socket(
            owner,
            &confirmation_id,
            "approve",
            Some(mode == "full_access"),
        )
        .await;
        assert_eq!(ack["op"], "ack", "{ack}");
        assert_eq!(ack["result"]["state"], "executing");
        // Creation is synchronous: the retry observes the recorded success receipt.
        let created = call(client, "request_create_agent", args.clone()).await;
        assert_eq!(created["status"], "succeeded", "{created}");
        assert_eq!(created["confirmation_id"], confirmation_id);
        let target = created["target_id"]
            .as_str()
            .ok_or("missing target")?
            .to_owned();
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
                !serde_json::to_string(&created)?.contains(forbidden),
                "AI result carries {forbidden}: {created}"
            );
        }
        let snapshot = store.snapshot("general", 0, 300).await?;
        let agent = snapshot
            .agent_sessions
            .iter()
            .find(|agent| agent.session_id == target)
            .ok_or("missing created agent")?;
        assert_eq!(
            agent.runtime_status,
            agentsassemble_domain::AgentRuntimeStatus::Stopped
        );
        assert!(!agent.enabled);
        assert_eq!(agent.permission_mode, mode);
        let participant = store.participant("general", &target).await?;
        assert_eq!(participant.owner_id, LOCAL_OPERATOR_PARTICIPANT_ID);
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
            "{created}"
        );
    }
    Ok(())
}
