use super::*;
use agentsassemble_domain::ParticipantStatus;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};

// One end-to-end pass for the owner-confirmed human kick on the connector entry: the
// pending request changes nothing and binds the target's admission identity, and the
// owner's approval runs the same participant-removal core and the same live-session
// revocation as the owner's own kick of a human, with delegate attribution.
pub(super) async fn verify(
    client: &Client,
    store: &SqliteStore,
    delegate: &str,
    connection: Option<&Value>,
    owner: (&str, &AppState),
) -> TestResult {
    let (base_url, _) = owner;
    // One removable human joins the room through a fresh invite, so the owner could
    // kick them themself at this moment.
    let http = reqwest::Client::new();
    let credentials = human_invite::persist_invite(
        store,
        InviteScope::ReadWrite,
        1,
        &Uuid::new_v4().to_string(),
        "Kick Target",
    )
    .await;
    let joined = human_invite::join(
        &http,
        base_url,
        credentials.invite_token(),
        &format!("aad1_{}", URL_SAFE_NO_PAD.encode([0xBB; 32])),
        &Uuid::new_v4().to_string(),
        "Kick Target",
        "",
    )
    .await;
    assert_eq!(joined["participant_type"], "human");
    let target = joined["agent_id"]
        .as_str()
        .ok_or("kick target missing")?
        .to_owned();
    assert_eq!(
        store.participant("general", &target).await?.status,
        ParticipantStatus::Joined
    );

    let mut args = json!({
        "request_id": Uuid::new_v4().to_string(),
        "participant_id": target,
    });
    if let Some(connection) = connection {
        args["connection_id"] = connection.clone();
    }
    let pending = call(client, "request_kick_participant", args.clone()).await;
    assert_eq!(pending["status"], "pending_confirmation", "{pending}");
    let confirmation_id = pending["confirmation_id"]
        .as_str()
        .ok_or("missing confirmation")?
        .to_owned();
    // Nothing changed: the target is still joined and no removal event exists yet.
    assert_eq!(
        store.participant("general", &target).await?.status,
        ParticipantStatus::Joined
    );
    assert!(
        !store
            .snapshot("general", 0, 400)
            .await?
            .events
            .iter()
            .any(|event| event.event_type == "participant_kicked"
                && event.participant_id.as_deref() == Some(target.as_str()))
    );
    assert_eq!(
        pending,
        call(client, "request_kick_participant", args.clone()).await
    );
    // Retargeting under the same request identity conflicts instead.
    let mut changed = args.clone();
    changed["participant_id"] = json!("missing");
    let conflict =
        room_portal_fixture::call_tool(client, "request_kick_participant", changed).await;
    assert_eq!(conflict.is_error, Some(true));
    assert!(
        conflict.content[0]
            .as_text()
            .unwrap()
            .text
            .contains("request_conflict"),
        "{conflict:?}"
    );
    // The owner approves through the room socket, so the room actor commits the removal
    // through the same core as the owner's own kick and closes the target's live session.
    let ack = super::decide_on_socket(owner, &confirmation_id, "approve", None).await;
    assert_eq!(ack["op"], "ack", "{ack}");
    assert_eq!(ack["result"]["state"], "succeeded", "{ack}");
    let done = call(client, "request_kick_participant", args).await;
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
        ParticipantStatus::Kicked
    );
    let snapshot = store.snapshot("general", 0, 400).await?;
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
            .any(|event| event.event_type == "participant_kicked"
                && event.participant_id.as_deref() == Some(target.as_str())),
        "{done}"
    );
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
