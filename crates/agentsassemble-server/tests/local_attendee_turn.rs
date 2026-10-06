//! Opt-in real provider reproduction; never enabled by the normal suite.
#![cfg(unix)]
use crate::support::{human_invite, local_socket};
use agentsassemble_domain::{LocalAttendeeCreate, LocalAttendeePhase};
use agentsassemble_persistence::SqliteStore;
use agentsassemble_provider::{ProviderAdapter, ProviderCatalogService, ProviderCredentialStore};
use agentsassemble_server::LocalAttendeeService;
use serde_json::json;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const ORIGIN: &str = "https://room.example.test";

#[tokio::test]
#[ignore = "requires explicit real Codex authorization"]
async fn real_codex_non_host_local_attendee_turn() -> Result<(), Box<dyn std::error::Error>> {
    Box::pin(real_codex_turns()).await
}

async fn real_codex_turns() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let credentials = ProviderCredentialStore::production();
    let catalog = ProviderCatalogService::discovering_selected("codex", &credentials)?;
    let mut updates = catalog.subscribe();
    tokio::time::timeout(std::time::Duration::from_secs(90), async {
        while updates.borrow_and_update().status == "loading" {
            updates.changed().await?;
        }
        Ok::<_, Box<dyn std::error::Error>>(())
    })
    .await??;
    catalog.shutdown().await?;
    let (store, invite, parent) = companion_fixture().await?;
    let server = human_invite::start(store.clone()).await;
    let local = SqliteStore::open("sqlite::memory:").await?;
    local
        .bootstrap_local_authority(&Uuid::new_v4().to_string(), "Local operator")
        .await?;
    local.restrict_hosting(false).await?;
    let service = LocalAttendeeService::new(local, CancellationToken::new());
    let adapter = ProviderAdapter::for_attendee(
        std::path::Path::new(env!("CARGO_BIN_EXE_agentsassemble-server")),
        directory.path(),
    );
    let id = Uuid::new_v4();
    let mut input = LocalAttendeeCreate {
        request_id: id,
        room_id: "general".into(),
        room_uid: invite.room_uid,
        invite_url: format!("{}/join?token={}", server.base_url, invite.invite_bearer),
        creation: json!({"provider_id":"codex", "display_name":"E2E6 reproduction", "workspace":"",
            "model":"gpt-5.6-luna", "reasoning_effort":"low", "start":true,
            "catalog_revision":catalog.snapshot().catalog_revision}),
    };
    let rejected = service
        .create(input.clone(), catalog.clone(), adapter.clone())
        .await
        .err()
        .ok_or("empty workspace accepted")?;
    assert_eq!(rejected.code, "invalid_workspace");
    eprintln!("E2E6: empty workspace rejected before admission");
    input.creation["workspace"] = json!(directory.path());
    let ready = service.create(input, catalog, adapter).await?;
    eprintln!("E2E6: phase={:?} code={:?}", ready.phase, ready.error_code);
    assert_eq!(ready.phase, LocalAttendeePhase::Running);
    let mut manager = local_socket::connect(&server.base_url, server.state(), "general").await;
    manager.subscribe(0).await;
    let _ = manager.receive_json().await;
    let outcome = tokio::time::timeout(std::time::Duration::from_mins(2), async {
        for turn in 1..=2 {
        manager.send_json(&json!({"op":"command", "request_id":format!("e2e6-input-{turn}"), "action":"message.send",
            "payload":{"content":format!("@E2E6 reproduction Please publish a short greeting containing the number {turn} to this room using the room tool.")}})).await;
        loop {
            let event = manager.receive_json().await;
            let current = service.status(id).await?;
            if current.phase != LocalAttendeePhase::Running {
                return Err(format!("local phase={:?} code={:?}", current.phase, current.error_code).into());
            }
            let snapshot = store.snapshot("general", 0, 200).await?;
            if snapshot.events.iter().filter(|e| e.event_type == "turn_finished").count() >= turn {
                let session = &snapshot.agent_sessions[0];
                eprintln!("E2E6: finished runtime={:?} code={} active={}", session.runtime_status, session.last_error_code, session.provider_session_active);
                if !session.last_error_code.is_empty() { return Err("room turn failed".into()); }
                let publications = snapshot.events.iter().filter(|event|
                    event.actor.participant_id == ready.participant_id.as_deref().unwrap_or_default()
                    && event.extra.get("message_source").and_then(serde_json::Value::as_str) == Some("room_portal")
                    && event.event_type == "message_final"
                    && event.content.as_ref().is_some_and(|content| !content.trim().is_empty())
                ).count();
                if publications < turn { return Err("turn completed without the requested room publication".into()); }
                store.authorize_operator_session(&parent, &[0x52; 32], ORIGIN).await?;
                break;
            }
            drop(event);
        }
        }
        Ok::<_, Box<dyn std::error::Error>>(())
    }).await;
    let status = service.status(id).await?;
    eprintln!(
        "E2E6: final phase={:?} code={:?}",
        status.phase, status.error_code
    );
    assert_eq!(service.cancel(id).await?.phase, LocalAttendeePhase::Stopped);
    service.shutdown().await?;
    assert!(!store.snapshot("general", 0, 200).await?.agent_sessions[0].provider_session_active);
    manager.close().await;
    server.stop().await;
    outcome??;
    Ok(())
}

async fn companion_fixture() -> Result<
    (
        SqliteStore,
        agentsassemble_persistence::AttendeeInvite,
        [u8; 32],
    ),
    Box<dyn std::error::Error>,
> {
    let (store, _) = human_invite::fixture(agentsassemble_domain::InviteScope::ReadWrite).await;
    let manager_authority = store
        .authorize_local_room_manager(
            "general",
            agentsassemble_domain::LOCAL_OPERATOR_USER_ID,
            agentsassemble_domain::LOCAL_OPERATOR_PARTICIPANT_ID,
        )
        .await?;
    let now = chrono::Utc::now();
    store
        .create_operator_pairing(
            &agentsassemble_persistence::RoomManagerAuthority::Local(manager_authority),
            &[0x51; 32],
            ORIGIN,
            now,
        )
        .await?;
    let paired = store
        .redeem_operator_pairing(&[0x51; 32], &[0x52; 32], ORIGIN, now)
        .await?;
    let parent = *paired.authorization.session_fingerprint();
    let invite = store
        .create_companion_attendee_invite(
            &agentsassemble_persistence::RoomSessionAuthorization::Operator(paired.authorization),
            agentsassemble_persistence::CompanionInviteRequest {
                request_id: Uuid::new_v4(),
                provider_kind: "codex_live_session",
                display_name: "E2E6 reproduction",
            },
            now,
        )
        .await?;
    Ok((store, invite, parent))
}
