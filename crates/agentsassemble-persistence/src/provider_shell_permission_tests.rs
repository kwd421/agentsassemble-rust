use super::{AGENT_ID, fixture, running_authority, save_stored_session, stored_session};
use crate::OpenProviderRequest;
use agentsassemble_domain::{ProviderRequest, ProviderRequestKind, ProviderRequestPrompt};
use serde_json::json;

#[tokio::test]
async fn shell_policy_uses_durable_mode_and_rejects_stale_execution()
-> Result<(), Box<dyn std::error::Error>> {
    let (store, principal, _directory) = fixture().await;
    let mutation = store
        .execute_message_with_turn(
            &principal,
            "shell-policy",
            "message.send",
            &json!({"content":"@Terra check shell policy"}),
        )
        .await?;
    let start = running_authority(&store, &mutation.assignments[0], "shell-policy-turn").await;
    let mut request = OpenProviderRequest {
        turn_generation: start.turn_generation,
        execution_id: start.execution_id.clone(),
        request: ProviderRequest {
            provider_request_id: uuid::Uuid::new_v4(),
            request_kind: ProviderRequestKind::Permission,
            title: "Shell permission".to_owned(),
            description: String::new(),
            timeout_seconds: 60,
            prompt: ProviderRequestPrompt::Acknowledge { action_url: None },
        },
    };
    for (mode, denied) in [("meeting_read_only", true), ("workspace_write", false)] {
        let mut session = stored_session(&store).await;
        session.public.permission_mode = mode.to_owned();
        save_stored_session(&store, &session).await;
        assert_eq!(
            store
                .managed_shell_permission_denied("general", AGENT_ID, &request)
                .await?,
            denied
        );
    }
    request.execution_id = uuid::Uuid::new_v4().to_string();
    assert!(
        store
            .managed_shell_permission_denied("general", AGENT_ID, &request)
            .await
            .is_err()
    );
    assert!(
        store
            .pending_provider_request_ids("general")
            .await?
            .is_empty()
    );
    Ok(())
}
