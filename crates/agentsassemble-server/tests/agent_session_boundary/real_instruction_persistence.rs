//! Run exactly one selected provider per process; never enabled in the normal suite.
//! Set `AA_VERIFY_PROVIDER` and optionally `AA_VERIFY_MODEL`. Native trace inspection
//! is required separately: passing ordinary turns does not prove compaction.
use super::*;
use agentsassemble_provider::ProviderCredentialStore;

#[tokio::test]
#[ignore = "requires explicit real-provider authorization and per-provider turn budget"]
async fn real_managed_instruction_persistence() -> Result<(), Box<dyn std::error::Error>> {
    let provider = std::env::var("AA_VERIFY_PROVIDER")?;
    assert!(matches!(provider.as_str(), "codex" | "opencode" | "claude"));
    let (catalog, model, effort) = discover_selection(&provider).await?;
    eprintln!("PERSIST selected model={model} effort={effort} max_budget=2 turns");
    let bash_denial = std::env::var_os("AA_VERIFY_BASH_DENIAL").is_some();
    let directory = tempfile::tempdir()?;
    let store = SqliteStore::open("sqlite::memory:").await?;
    bootstrap(&store).await;
    let server = start(store.clone(), catalog).await;
    let outcome = tokio::time::timeout(Duration::from_mins(7), async {
        let mut socket = connect(&server.base_url, &server.state).await;
        subscribe(&mut socket).await;
        let _ = receive_json(&mut socket).await;
        let mut payload = json!({
            "provider_id":provider, "catalog_revision":server.state.provider_catalog.snapshot().catalog_revision,
            "display_name":"PersistenceProbe", "workspace":directory.path(),
            "model":model, "permission_mode":"meeting_read_only", "start":true,
        });
        if !effort.is_empty() { payload["reasoning_effort"] = json!(effort); }
        send_create(&mut socket, "persistence-create", &payload).await;
        let created = receive_command_ack(&mut socket).await;
        let session = &created["result"]["start"]["agent_session"];
        eprintln!("PERSIST start runtime={} code={}", session["runtime_status"], session["last_error_code"]);
        if session["runtime_status"] != "idle" { return Err("provider start failed".into()); }
        let session_id = session["session_id"].as_str().ok_or("session ID absent")?.to_owned();
        for turn in 1..=if bash_denial { 1 } else { 2 } {
            if turn == 2 && provider == "claude" && std::env::var_os("AA_VERIFY_COMPACT").is_none() {
                for action in ["agent.stop", "agent.resume"] {
                    send_command(&mut socket, &format!("persistence-{action}"), action,
                        &json!({"session_id":session_id})).await;
                    let ack = receive_command_ack(&mut socket).await;
                    eprintln!("PERSIST lifecycle={action} runtime={} code={} reused={}",
                        ack["result"]["agent_session"]["runtime_status"],
                        ack["result"]["agent_session"]["last_error_code"], ack["result"]["agent_session"]["provider_session_reused"]);
                    if action == "agent.resume" && ack["result"]["agent_session"]["runtime_status"] != "idle" {
                        return Err("provider resume failed".into());
                    }
                }
            }
            let content = if bash_denial {
                "@PersistenceProbe For this permission verification, attempt the native bash tool exactly once with command `echo hi > bash-executed.txt` in the current scratch workspace. Do not create that file using any other tool. If permission is denied, do not retry or work around it; publish a short report of the denial.".to_owned()
            } else {
                format!("@PersistenceProbe Say persistence-check-{turn} in one short sentence.")
            };
            send_command(&mut socket, &format!("persistence-message-{turn}"), "message.send",
                &json!({"content":content})).await;
            loop {
                let _ = receive_json_with_timeout(&mut socket, Duration::from_mins(4)).await;
                let snapshot = store.snapshot("general", 0, 200).await?;
                if bash_denial && snapshot.events.iter().any(|event| event.event_type == "provider_request_opened") {
                    return Err("bash policy unexpectedly opened a user prompt".into());
                }
                let agent = snapshot.agent_sessions.iter().find(|item| item.session_id == session_id)
                    .ok_or("agent disappeared")?;
                if !agent.last_error_code.is_empty() {
                    eprintln!("PERSIST turn={turn} error_code={}", agent.last_error_code);
                    if provider == "opencode" {
                        let korean = agent.last_error.contains("OpenCode")
                            && agent.last_error.chars().any(|c| ('가'..='힣').contains(&c));
                        eprintln!("PERSIST provider_error_has_korean_guidance={korean}");
                        assert!(korean, "OpenCode failures must expose Korean guidance");
                    }
                    return Err("provider turn failed".into());
                }
                let finished = snapshot.events.iter().filter(|event| event.event_type == "turn_finished").count();
                if finished >= turn {
                    let publications = snapshot.events.iter().filter(|event|
                        event.event_type == "message_final" && event.actor.participant_id == session_id
                        && event.extra.get("message_source").and_then(Value::as_str) == Some("room_portal")
                    ).count();
                    // The portal requires a fresh read_discussion receipt before publication.
                    // Native traces must separately establish first-call order and compaction.
                    eprintln!("PERSIST turn={turn} finished={finished} portal_publications={publications} error_code={}", agent.last_error_code);
                    if !bash_denial && publications != turn { return Err("room publication missing".into()); }
                    if bash_denial {
                        assert!(!directory.path().join("bash-executed.txt").exists());
                        assert!(store.pending_provider_request_ids("general").await?.is_empty());
                        assert!(!snapshot.events.iter().any(|event| event.event_type == "provider_request_opened"));
                        eprintln!("PERSIST bash_sentinel_absent=true user_prompt_opened=false");
                    }
                    if turn == 1 && !bash_denial && provider == "opencode"
                        && std::env::var_os("AA_VERIFY_COMPACT").is_some()
                    {
                        let result = tokio::process::Command::new("python3")
                            .arg("-B")
                            .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/agent_session_boundary/run_opencode_summarize_fixture.py"))
                            .arg(directory.path())
                            .status().await?;
                        if !result.success() { return Err("native summarize failed".into()); }
                    }
                    break;
                }
            }
        }
        Ok::<_, Box<dyn std::error::Error>>(())
    }).await;
    server.stop_and_close().await;
    outcome??;
    Ok(())
}

async fn discover_selection(
    provider: &str,
) -> Result<(ProviderCatalog, String, &'static str), Box<dyn std::error::Error>> {
    let discovery = ProviderCatalogService::discovering_selected(
        provider,
        &ProviderCredentialStore::production(),
    )?;
    let mut updates = discovery.subscribe();
    let discovered = tokio::time::timeout(Duration::from_secs(90), async {
        while updates.borrow_and_update().status == "loading" {
            updates.changed().await?;
        }
        Ok::<_, Box<dyn std::error::Error>>(discovery.snapshot())
    })
    .await;
    discovery.shutdown().await?;
    let catalog = discovered??;
    let available = catalog
        .providers
        .iter()
        .find(|item| item.id == provider)
        .ok_or("provider absent")?;
    eprintln!(
        "PERSIST provider={provider} available={} code={}",
        available.available, available.discovery_error_code
    );
    if !available.available {
        return Err("provider unavailable".into());
    }
    let models = available
        .controls
        .iter()
        .find(|control| control.key == "model")
        .ok_or("models absent")?;
    let model = std::env::var("AA_VERIFY_MODEL").unwrap_or_else(|_| {
        if provider == "claude" {
            models
                .options
                .iter()
                .find(|option| option.value.starts_with("claude-sonnet-"))
                .map_or_else(String::new, |option| option.value.clone())
        } else {
            available.default_model.clone()
        }
    });
    if !models.options.iter().any(|option| option.value == model) {
        return Err("requested model absent".into());
    }
    if provider == "claude" && !model.starts_with("claude-sonnet-") {
        return Err("this verification authorizes Sonnet only".into());
    }
    let effort = if provider == "claude" || provider == "codex" {
        "low"
    } else {
        ""
    };
    Ok((catalog, model, effort))
}
