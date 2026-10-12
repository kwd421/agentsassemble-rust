use super::*;
use agentsassemble_domain::{
    AuthenticatedPrincipal, CapabilitySet, ClientKind, InviteScope, LOCAL_OPERATOR_PARTICIPANT_ID,
    LOCAL_OPERATOR_USER_ID,
};
use agentsassemble_persistence::{RoomManagerAuthority, RoomMutationAuthority};
use sha2::{Digest, Sha256};
use uuid::Uuid;

#[tokio::test]
async fn delegated_admission_shares_owner_and_delegate_windows_and_replay_is_free()
-> Result<(), Box<dyn std::error::Error>> {
    let store = SqliteStore::open("sqlite::memory:").await?;
    store
        .bootstrap_local_authority(&Uuid::new_v4().to_string(), "Owner")
        .await?;
    store
        .create_room_for_local_operator(&Uuid::new_v4().to_string(), "room", "Room")
        .await?;
    let manager = store
        .authorize_local_room_manager(
            "room",
            LOCAL_OPERATOR_USER_ID,
            LOCAL_OPERATOR_PARTICIPANT_ID,
        )
        .await?;
    let principal = AuthenticatedPrincipal {
        principal_id: LOCAL_OPERATOR_USER_ID.into(),
        participant_id: LOCAL_OPERATOR_PARTICIPANT_ID.into(),
        display_name: "Owner".into(),
        room_id: "room".into(),
        client_kind: ClientKind::Browser,
        invite_scope: InviteScope::ReadWrite,
        is_operator: true,
        capabilities: CapabilitySet::local_operator(ClientKind::Browser, InviteScope::ReadWrite),
    };
    let message = store
        .execute_message(
            &principal,
            "target",
            "message.send",
            &json!({"content":"Target"}),
        )
        .await?;
    let mut delegates = Vec::new();
    for index in 1..=2 {
        let invite = store
            .create_connector_invite(
                &RoomManagerAuthority::Local(manager.clone()),
                Uuid::new_v4(),
                InviteScope::ReadWrite,
                chrono::Utc::now(),
            )
            .await?;
        let joined = store
            .admit_connector(
                &Sha256::digest(invite.invite_bearer.as_bytes()).into(),
                &[index; 32],
                Uuid::new_v4(),
                "AI",
                chrono::Utc::now(),
            )
            .await?;
        store.execute_participant_role_update(RoomMutationAuthority::TrustedPrincipal(&principal), &format!("role-{index}"), &json!({"participant_id":joined.authorization.principal().participant_id,"role":"director"})).await?;
        delegates.push(DelegateRoomAuthority::Connector(joined.authorization));
    }
    let admission = PrincipalMutationAdmission::new();
    let mut request = SetMessagePin {
        channel_id: "lobby".into(),
        message_id: message.event.id,
        pinned: true,
        request_id: Uuid::new_v4().to_string(),
    };
    let first = apply(&store, &admission, &delegates[0], &request).await?;
    let target_directory = tempfile::tempdir()?;
    let target_id = "codex-00000000-0000-5000-8000-000000000321";
    let draft = crate::runtime_reconciliation::tests::draft(target_directory.path(), target_id);
    let target_id = draft.agent_id.as_str();
    store
        .execute_agent_create(
            RoomMutationAuthority::TrustedPrincipal(&principal),
            &Uuid::new_v4().to_string(),
            &json!({"display_name":"Budget target"}),
            &draft,
        )
        .await?;
    let direct = agentsassemble_domain::DirectRoomAction::CreateTextChannel(
        agentsassemble_domain::CreateTextChannel {
            request_id: Uuid::new_v4().to_string(),
            name: "Planning".into(),
        },
    );
    let provider = agentsassemble_provider::ProviderAdapter::new();
    let tracker = crate::lifecycle_command_tracker::LifecycleCommandTracker::default();
    let created = super::super::delegated_direct::apply(
        &store,
        &provider,
        &admission,
        &delegates[0],
        &direct,
        &tracker,
        None,
    )
    .await
    .execution
    .reply
    .map_err(|failure| failure.error)?;
    assert_eq!(
        created.event.extra["on_behalf_of_participant_id"],
        LOCAL_OPERATOR_PARTICIPANT_ID
    );
    // Ordinary human admission uses this same principal window. Exhaust it by
    // debiting distinct normal requests; the next AI cannot gain a separate allowance.
    for index in 0..10_000 {
        if admission
            .charge(
                LOCAL_OPERATOR_USER_ID,
                MutationIdentity::new("room", &format!("human-{index}"), "message.send", "hash"),
                1,
            )
            .is_err()
        {
            break;
        }
    }
    let retry = apply(&store, &admission, &delegates[0], &request).await?;
    assert!(retry.deduplicated);
    assert_eq!(retry.result, first.result);
    let replay = super::super::delegated_direct::apply(
        &store,
        &provider,
        &admission,
        &delegates[0],
        &direct,
        &tracker,
        None,
    )
    .await
    .execution
    .reply
    .map_err(|failure| failure.error)?;
    assert!(replay.deduplicated);
    assert_eq!(replay.result, created.result);
    for delegate in &delegates {
        let next = agentsassemble_domain::DirectRoomAction::CreateTextChannel(
            agentsassemble_domain::CreateTextChannel {
                request_id: Uuid::new_v4().to_string(),
                name: "Denied".into(),
            },
        );
        for next in [
            next,
            agentsassemble_domain::DirectRoomAction::StartAgent(
                agentsassemble_domain::StartAgent {
                    request_id: Uuid::new_v4().to_string(),
                    agent_id: target_id.into(),
                },
            ),
            agentsassemble_domain::DirectRoomAction::StopAgent(agentsassemble_domain::StopAgent {
                request_id: Uuid::new_v4().to_string(),
                agent_id: target_id.into(),
            }),
        ] {
            let result = super::super::delegated_direct::apply(
                &store, &provider, &admission, delegate, &next, &tracker, None,
            )
            .await
            .execution
            .reply
            .map_err(|failure| failure.error);
            assert!(
                matches!(&result,
                Err(PersistenceError::CommandRejected { code, .. }) if code == "write_budget_exceeded"),
                "{result:?}"
            );
        }
    }
    request.request_id = Uuid::new_v4().to_string();
    for delegate in &delegates {
        assert!(
            matches!(apply(&store, &admission, delegate, &request).await,
            Err(PersistenceError::CommandRejected { code, .. }) if code == "write_budget_exceeded")
        );
    }
    // A fresh owner budget cannot bypass the delegate's own existing window.
    let admission = PrincipalMutationAdmission::new();
    let DelegateRoomAuthority::Connector(delegate) = &delegates[0] else {
        unreachable!()
    };
    for index in 0..10_000 {
        if admission
            .charge(
                &delegate.principal().principal_id,
                MutationIdentity::new("room", &format!("ai-{index}"), "message.send", "hash"),
                1,
            )
            .is_err()
        {
            break;
        }
    }
    assert!(
        matches!(apply(&store, &admission, &delegates[0], &request).await,
        Err(PersistenceError::CommandRejected { code, .. }) if code == "write_budget_exceeded")
    );
    assert_eq!(store.local_message_pins(&manager, "lobby").await?.len(), 1);
    Ok(())
}
