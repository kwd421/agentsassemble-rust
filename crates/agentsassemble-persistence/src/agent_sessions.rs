use agentsassemble_domain::{AgentSessionDraft, canonical_payload_hash};
use serde_json::Value;

use crate::{
    CommandOutcome, PersistenceError, RoomMutationAuthority, SqliteStore,
    agent_creation_records::create_or_reuse_agent_records,
    agent_lifecycle_authority::authorize_control,
    agent_lifecycle_events::store_result,
    authority::active_room_for_principal,
    command_admission::{admit_non_lifecycle_command, inspect_non_lifecycle_command},
    filesystem_authority::revalidate_runtime_authority,
    room_write_budget::command_size,
};

impl SqliteStore {
    /// Returns a committed result before a caller consults mutable external selection state.
    ///
    /// # Errors
    ///
    /// Returns a conflict, inactive-session rejection, or persistence failure.
    pub async fn replay_command(
        &self,
        authority: RoomMutationAuthority<'_>,
        request_id: &str,
        action: &str,
        payload: &Value,
    ) -> Result<Option<CommandOutcome>, PersistenceError> {
        let payload_hash = canonical_payload_hash(payload);
        let mut transaction = self.pool.begin().await?;
        let principal = authority.resolve(&mut transaction).await?;
        let principal = principal.as_ref();
        active_room_for_principal(&mut transaction, principal).await?;
        let outcome = inspect_non_lifecycle_command(
            &mut transaction,
            &principal.room_id,
            &principal.principal_id,
            request_id,
            action,
            &payload_hash,
        )
        .await?;
        if outcome.is_some() {
            authority.record_success(&mut transaction).await?;
        }
        transaction.commit().await?;
        Ok(outcome)
    }

    /// Atomically creates a stopped, server-owned Agent Session and its room event.
    ///
    /// # Errors
    ///
    /// Returns authorization, identity, idempotency, or persistence failures.
    pub async fn execute_agent_create(
        &self,
        authority: RoomMutationAuthority<'_>,
        request_id: &str,
        payload: &Value,
        draft: &AgentSessionDraft,
    ) -> Result<CommandOutcome, PersistenceError> {
        const ACTION: &str = "agent.create";
        let payload_hash = canonical_payload_hash(payload);
        {
            let mut transaction = self.pool.begin().await?;
            let principal = authority.resolve(&mut transaction).await?;
            let principal = principal.as_ref();
            authorize_control(principal)?;
            active_room_for_principal(&mut transaction, principal).await?;
            let outcome = inspect_non_lifecycle_command(
                &mut transaction,
                &principal.room_id,
                &principal.principal_id,
                request_id,
                ACTION,
                &payload_hash,
            )
            .await?;
            if outcome.is_some() {
                authority.record_success(&mut transaction).await?;
            }
            transaction.commit().await?;
            if let Some(outcome) = outcome {
                return Ok(outcome);
            }
        }
        revalidate_runtime_authority(draft).await?;
        let mut transaction = self.pool.begin().await?;
        let principal = authority.resolve(&mut transaction).await?;
        let principal = principal.as_ref();
        authorize_control(principal)?;
        active_room_for_principal(&mut transaction, principal).await?;
        if let Some(outcome) = admit_non_lifecycle_command(
            &mut transaction,
            &principal.room_id,
            &principal.principal_id,
            request_id,
            ACTION,
            &payload_hash,
            command_size(request_id, ACTION, payload)?,
        )
        .await?
        {
            authority.record_success(&mut transaction).await?;
            transaction.commit().await?;
            return Ok(outcome);
        }
        let records =
            create_or_reuse_agent_records(&mut transaction, principal, draft, None, false).await?;
        let outcome = store_result(
            &mut transaction,
            principal,
            request_id,
            ACTION,
            payload_hash,
            records.result,
            records.committed_events,
        )
        .await?;
        authority.record_success(&mut transaction).await?;
        transaction.commit().await?;
        Ok(outcome)
    }
}

#[cfg(test)]
mod tests {
    use crate::RoomMutationAuthority::TrustedPrincipal;
    use std::{collections::BTreeMap, fs::File, path::Path};

    use agentsassemble_domain::{
        AgentSessionDraft, AuthenticatedPrincipal, CapabilitySet, ClientKind, DurableAgentSession,
        InviteScope, LOCAL_OPERATOR_PARTICIPANT_ID, PersonaAssetKind, PersonaCard,
        PersonaLoreSettings, stable_content_identity, stable_identity_hash,
    };
    use same_file::Handle;
    use serde_json::json;

    use crate::{ImportedPersonaAsset, PersistenceError, SqliteStore};

    #[tokio::test]
    async fn paired_creation_configuration_and_replay_stop_at_session_revocation() {
        let (store, principal, directory) = fixture().await;
        let manager = store
            .authorize_local_room_manager(
                &principal.room_id,
                &principal.principal_id,
                &principal.participant_id,
            )
            .await
            .unwrap_or_else(|error| panic!("manager: {error}"));
        let now = chrono::Utc::now();
        let origin = "https://room.example.test";
        let pairing = store
            .create_operator_pairing(
                &crate::RoomManagerAuthority::Local(manager.clone()),
                &[1; 32],
                origin,
                now,
            )
            .await
            .unwrap_or_else(|error| panic!("pairing: {error}"));
        let redeemed = store
            .redeem_operator_pairing(&[1; 32], &[2; 32], origin, now)
            .await
            .unwrap_or_else(|error| panic!("redeem: {error}"));
        let authority = crate::RoomMutationAuthority::OperatorSession(&redeemed.authorization);
        let session = draft(directory.path().to_str().unwrap_or_else(|| panic!("path")));
        let payload = json!({"provider_id": "api"});
        store
            .execute_agent_create(authority, "paired-create", &payload, &session)
            .await
            .unwrap_or_else(|error| panic!("create: {error}"));
        let configuration = json!({"agent_id": session.agent_id});
        store
            .agent_configuration_candidate(authority, &configuration)
            .await
            .unwrap_or_else(|error| panic!("candidate: {error}"));
        store
            .execute_agent_configuration(
                authority,
                "paired-configure",
                &configuration,
                &session.runtime_profile_key,
                &session,
            )
            .await
            .unwrap_or_else(|error| panic!("configure: {error}"));
        store
            .revoke_operator_pairing(
                &crate::RoomManagerAuthority::Local(manager.clone()),
                pairing.pairing_id,
            )
            .await
            .unwrap_or_else(|error| panic!("revoke: {error}"));
        let errors = [
            store
                .execute_agent_create(authority, "paired-create", &payload, &session)
                .await
                .err(),
            store
                .replay_command(authority, "paired-create", "agent.create", &payload)
                .await
                .err(),
            store
                .agent_configuration_candidate(authority, &configuration)
                .await
                .err(),
            store
                .execute_agent_configuration(
                    authority,
                    "paired-configure",
                    &configuration,
                    &session.runtime_profile_key,
                    &session,
                )
                .await
                .err(),
        ];
        for error in errors {
            assert!(matches!(
                error,
                Some(PersistenceError::CommandRejected { code, .. }) if matches!(code.as_bytes(), b"session_revoked")
            ));
        }
    }

    async fn fixture() -> (SqliteStore, AuthenticatedPrincipal, tempfile::TempDir) {
        let directory =
            tempfile::tempdir().unwrap_or_else(|error| panic!("create test directory: {error}"));
        let store = SqliteStore::open_path(&directory.path().join("runtime.sqlite3"))
            .await
            .unwrap_or_else(|error| panic!("open store: {error}"));
        store
            .bootstrap_local_authority("42aebf93-31ce-46fd-b792-0a791b644668", "Host")
            .await
            .unwrap_or_else(|error| panic!("bootstrap identity: {error}"));
        store
            .create_room_for_local_operator(
                "20000000-0000-4000-8000-000000000010",
                "general",
                "General",
            )
            .await
            .unwrap_or_else(|error| panic!("create room: {error}"));
        let principal = AuthenticatedPrincipal {
            principal_id: "operator-local-user".to_owned(),
            participant_id: LOCAL_OPERATOR_PARTICIPANT_ID.to_owned(),
            display_name: "Host".to_owned(),
            room_id: "general".to_owned(),
            client_kind: ClientKind::Browser,
            invite_scope: InviteScope::ReadWrite,
            is_operator: true,
            capabilities: CapabilitySet::local_operator(
                ClientKind::Browser,
                InviteScope::ReadWrite,
            ),
        };
        (store, principal, directory)
    }

    fn draft(workspace: &str) -> AgentSessionDraft {
        let workspace = std::fs::canonicalize(workspace)
            .unwrap_or_else(|error| panic!("canonical workspace: {error}"));
        let executable = std::env::current_exe()
            .and_then(std::fs::canonicalize)
            .unwrap_or_else(|error| panic!("canonical executable: {error}"));
        let (executable, executable_identity) = executable_authority(&executable);
        AgentSessionDraft {
            agent_id: "codex-00000000-0000-5000-8000-000000000001".to_owned(),
            display_name: "Terra".to_owned(),
            provider_kind: "opencode_server".to_owned(),
            runtime_kind: "opencode".to_owned(),
            connection_kind: "native_cli_bridge".to_owned(),
            executable,
            executable_identity,
            workspace: workspace.to_string_lossy().into_owned(),
            workspace_identity: stable_identity_hash(
                &Handle::from_path(&workspace)
                    .unwrap_or_else(|error| panic!("open workspace: {error}")),
            ),
            provider_endpoint: String::new(),
            model: "gpt-5.6-terra".to_owned(),
            reasoning_effort: "medium".to_owned(),
            service_tier: "default".to_owned(),
            variant: String::new(),
            execution_harness: "builtin".to_owned(),
            permission_mode: "meeting_read_only".to_owned(),
            max_output_tokens: 0,
            catalog_revision: "catalog-1".to_owned(),
            persona_card_id: String::new(),
            runtime_profile_key: "profile-1".to_owned(),
            transport: "stdio_jsonl".to_owned(),
        }
    }

    fn executable_authority(executable: &Path) -> (String, String) {
        let executable = executable
            .canonicalize()
            .unwrap_or_else(|error| panic!("canonical executable authority: {error}"));
        let mut file =
            File::open(&executable).unwrap_or_else(|error| panic!("open executable: {error}"));
        let handle = Handle::from_file(
            file.try_clone()
                .unwrap_or_else(|error| panic!("clone executable: {error}")),
        )
        .unwrap_or_else(|error| panic!("identify executable: {error}"));
        let identity = stable_content_identity(&handle, &mut file)
            .unwrap_or_else(|error| panic!("hash executable: {error}"));
        (executable.to_string_lossy().into_owned(), identity)
    }

    fn make_executable(executable: &Path) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let mut permissions = std::fs::metadata(executable)
                .unwrap_or_else(|error| panic!("read executable permissions: {error}"))
                .permissions();
            permissions.set_mode(0o700);
            std::fs::set_permissions(executable, permissions)
                .unwrap_or_else(|error| panic!("set executable permissions: {error}"));
        }
        #[cfg(not(unix))]
        let _ = executable;
    }

    #[tokio::test]
    async fn create_replay_and_snapshot_are_one_durable_identity() {
        let (store, principal, directory) = fixture().await;
        let payload = json!({"provider_id": "codex", "catalog_revision": "catalog-1"});
        let workspace = directory
            .path()
            .to_str()
            .unwrap_or_else(|| panic!("test workspace path must be UTF-8"));
        let session = draft(workspace);
        let first = store
            .execute_agent_create(TrustedPrincipal(&principal), "create-1", &payload, &session)
            .await
            .unwrap_or_else(|error| panic!("create session: {error}"));
        let retry = store
            .execute_agent_create(TrustedPrincipal(&principal), "create-1", &payload, &session)
            .await
            .unwrap_or_else(|error| panic!("retry session: {error}"));
        assert!(!first.deduplicated);
        assert!(retry.deduplicated);
        assert_eq!(first.event.id, retry.event.id);
        for private in [
            "workspace",
            "workspace_identity",
            "executable",
            "executable_identity",
            "provider_endpoint",
            "runtime_profile_key",
            "runtime_profile_version",
            "provider_session_id",
            "runtime_handle_id",
            "runtime_lease_token",
            "runtime_owner_id",
            "lifecycle_intent_action",
            "lifecycle_intent_id",
            "lifecycle_intent_status",
        ] {
            assert!(first.result["agent_session"].get(private).is_none());
            assert!(retry.result["agent_session"].get(private).is_none());
        }
        let durable = sqlx::query_scalar::<_, String>(
            "SELECT session_json FROM agent_sessions WHERE room_id = 'general' LIMIT 1",
        )
        .fetch_one(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("read durable session: {error}"));
        let durable: serde_json::Value = serde_json::from_str(&durable)
            .unwrap_or_else(|error| panic!("decode durable session: {error}"));
        assert_eq!(durable["workspace"], session.workspace);
        assert_eq!(durable["executable"], session.executable);
        assert!(matches!(
            store
                .replay_command(
                    TrustedPrincipal(&principal),
                    "create-1",
                    "agent.create",
                    &json!({"provider_id": "changed"})
                )
                .await,
            Err(PersistenceError::CommandConflict)
        ));
        let snapshot = store
            .snapshot("general", 0, 200)
            .await
            .unwrap_or_else(|error| panic!("snapshot: {error}"));
        assert_eq!(snapshot.agent_sessions.len(), 1);
        assert_eq!(snapshot.agent_sessions[0].model, "gpt-5.6-terra");
        assert_eq!(snapshot.events[0].event_type, "room_created");
        assert_eq!(snapshot.events[1].event_type, "agent_session_created");
        let created = &snapshot.events[1];
        assert_eq!(created.extra["participant"]["status"], "detached");
        assert_eq!(
            created.extra["participant"]["owner_id"],
            principal.participant_id
        );
        assert_eq!(
            created.extra["participant"]["participant_id"],
            session.agent_id
        );
        assert_eq!(
            created.extra["agent_session"]["session_id"],
            session.agent_id
        );
        assert_eq!(created.extra["agent_session"]["runtime_status"], "stopped");
        assert_eq!(created.extra["agent_session"]["enabled"], false);
        for private in [
            "executable",
            "workspace",
            "runtime_profile_key",
            "provider_session_id",
            "runtime_handle_id",
            "runtime_lease_token",
            "runtime_owner_id",
            "lifecycle_intent_action",
        ] {
            assert!(created.extra["agent_session"].get(private).is_none());
        }
    }

    #[tokio::test]
    async fn permission_downgrade_discards_native_session_atomically_and_replay_preserves_rebind() {
        for (old, new) in [
            ("workspace_write", "meeting_read_only"),
            ("full_access", "workspace_write"),
            ("full_access", "meeting_read_only"),
            ("workspace_write", "full_access"),
            ("meeting_read_only", "full_access"),
        ] {
            check_permission_reconfiguration(old, new).await;
        }
    }

    async fn check_permission_reconfiguration(old: &str, new: &str) {
        let (store, principal, directory) = fixture().await;
        let mut selected = draft(directory.path().to_str().unwrap_or_else(|| panic!("path")));
        selected.permission_mode = old.into();
        store
            .execute_agent_create(
                TrustedPrincipal(&principal),
                "create-write",
                &json!({}),
                &selected,
            )
            .await
            .unwrap_or_else(|error| panic!("create: {error}"));
        let payload = json!({"agent_id": selected.agent_id, "permission_mode": new});
        let mut session = store
            .agent_configuration_candidate(TrustedPrincipal(&principal), &payload)
            .await
            .unwrap_or_else(|error| panic!("candidate: {error}"));
        session.provider_session_id = "native-with-old-write-grants".into();
        persist_native_session_fixture(&store, &session).await;

        // An unrelated settings update must retain the native conversation.
        store
            .execute_agent_configuration(
                TrustedPrincipal(&principal),
                "unchanged-mode",
                &json!({"agent_id": selected.agent_id}),
                &selected.runtime_profile_key,
                &selected,
            )
            .await
            .unwrap_or_else(|error| panic!("permission downgrade fixture: {error}"));
        let unchanged = store
            .agent_configuration_candidate(TrustedPrincipal(&principal), &payload)
            .await
            .unwrap_or_else(|error| panic!("permission downgrade fixture: {error}"));
        assert_eq!(unchanged.provider_session_id, session.provider_session_id);

        let mut downgraded = selected.clone();
        downgraded.permission_mode = new.into();
        downgraded.runtime_profile_key = "profile-read-only".into();
        sqlx::query("CREATE TRIGGER reject_configuration_result BEFORE INSERT ON command_results BEGIN SELECT RAISE(ABORT, 'injected failure'); END")
            .execute(&store.pool).await.unwrap_or_else(|error| panic!("permission downgrade fixture: {error}"));
        assert!(
            store
                .execute_agent_configuration(
                    TrustedPrincipal(&principal),
                    "downgrade",
                    &payload,
                    &selected.runtime_profile_key,
                    &downgraded,
                )
                .await
                .is_err()
        );
        let rolled_back = store
            .agent_configuration_candidate(TrustedPrincipal(&principal), &payload)
            .await
            .unwrap_or_else(|error| panic!("permission downgrade fixture: {error}"));
        assert_eq!(rolled_back.public.permission_mode, old);
        assert_eq!(rolled_back.provider_session_id, session.provider_session_id);
        sqlx::query("DROP TRIGGER reject_configuration_result")
            .execute(&store.pool)
            .await
            .unwrap_or_else(|error| panic!("permission downgrade fixture: {error}"));

        store
            .execute_agent_configuration(
                TrustedPrincipal(&principal),
                "downgrade",
                &payload,
                &selected.runtime_profile_key,
                &downgraded,
            )
            .await
            .unwrap_or_else(|error| panic!("permission downgrade fixture: {error}"));
        let mut fresh = store
            .agent_configuration_candidate(TrustedPrincipal(&principal), &payload)
            .await
            .unwrap_or_else(|error| panic!("permission downgrade fixture: {error}"));
        assert_eq!(fresh.public.permission_mode, new);
        assert_eq!(fresh.public.session_id, session.public.session_id);
        assert!(
            fresh.provider_session_id.is_empty(),
            "next start must create, not resume, a native session"
        );

        fresh.provider_session_id = "new-read-only-native-session".into();
        persist_native_session_fixture(&store, &fresh).await;
        store
            .execute_agent_configuration(
                TrustedPrincipal(&principal),
                "downgrade",
                &payload,
                &selected.runtime_profile_key,
                &downgraded,
            )
            .await
            .unwrap_or_else(|error| panic!("permission downgrade fixture: {error}"));
        let replayed = store
            .agent_configuration_candidate(TrustedPrincipal(&principal), &payload)
            .await
            .unwrap_or_else(|error| panic!("permission downgrade fixture: {error}"));
        assert_eq!(replayed.provider_session_id, fresh.provider_session_id);
    }

    async fn persist_native_session_fixture(store: &SqliteStore, session: &DurableAgentSession) {
        let mut transaction = store
            .pool
            .begin()
            .await
            .unwrap_or_else(|error| panic!("permission downgrade fixture: {error}"));
        crate::agent_lifecycle::save_session(&mut transaction, session)
            .await
            .unwrap_or_else(|error| panic!("permission downgrade fixture: {error}"));
        transaction
            .commit()
            .await
            .unwrap_or_else(|error| panic!("permission downgrade fixture: {error}"));
    }

    #[tokio::test]
    async fn oversized_stored_persona_cannot_mutate_session_authority() {
        let (store, principal, directory) = fixture().await;
        let selected = draft(
            directory
                .path()
                .to_str()
                .unwrap_or_else(|| panic!("workspace")),
        );
        store
            .execute_agent_create(
                TrustedPrincipal(&principal),
                "create-valid",
                &json!({"provider_id":"api"}),
                &selected,
            )
            .await
            .unwrap_or_else(|error| panic!("create valid session: {error}"));
        // A pre-existing oversized card must not enter either create or configure.
        let oversized = persona_card("oversized", &"A".repeat(300_000));
        sqlx::query("INSERT INTO persona_assets(persona_id, card_json) VALUES (?, ?)")
            .bind(&oversized.id)
            .bind(
                serde_json::to_string(&oversized)
                    .unwrap_or_else(|error| panic!("oversized persona fixture: {error}")),
            )
            .execute(&store.pool)
            .await
            .unwrap_or_else(|error| panic!("oversized persona fixture: {error}"));
        let before = store
            .snapshot("general", 0, 200)
            .await
            .unwrap_or_else(|error| panic!("oversized persona fixture: {error}"));
        let mut rejected = selected.clone();
        rejected.persona_card_id = "oversized".into();
        rejected.runtime_profile_key = "oversized-profile".into();
        assert!(
            store
                .execute_agent_configuration(
                    TrustedPrincipal(&principal),
                    "configure-oversized",
                    &json!({"persona_card_id":"oversized"}),
                    &selected.runtime_profile_key,
                    &rejected
                )
                .await
                .is_err()
        );
        rejected.agent_id = "new-oversized".into();
        assert!(
            store
                .execute_agent_create(
                    TrustedPrincipal(&principal),
                    "create-oversized",
                    &json!({"persona_card_id":"oversized"}),
                    &rejected
                )
                .await
                .is_err()
        );
        let after = store
            .snapshot("general", 0, 200)
            .await
            .unwrap_or_else(|error| panic!("oversized persona fixture: {error}"));
        assert_eq!(before.agent_sessions, after.agent_sessions);
        assert_eq!(before.last_seq, after.last_seq);
    }

    #[tokio::test]
    async fn persona_selection_create_failure_and_clear_share_session_custody() {
        let (store, principal, directory) = fixture().await;
        store
            .replace_persona_asset(ImportedPersonaAsset {
                card: persona_card("guide", "Guide"),
                thumbnail: None,
            })
            .await
            .unwrap_or_else(|error| panic!("store persona: {error}"));
        let workspace = directory
            .path()
            .to_str()
            .unwrap_or_else(|| panic!("test workspace path must be UTF-8"));
        let mut selected = draft(workspace);
        selected.persona_card_id = "guide".to_owned();
        selected.runtime_profile_key = "profile-guide".to_owned();
        let created = store
            .execute_agent_create(
                TrustedPrincipal(&principal),
                "create-persona",
                &json!({"provider_id": "api", "persona_card_id": "guide"}),
                &selected,
            )
            .await
            .unwrap_or_else(|error| panic!("create selected session: {error}"));
        let projection = &created.result["agent_session"];
        assert_eq!(projection["persona_card_id"], "guide");
        assert_eq!(projection["persona_card"]["display_name"], "Guide");
        assert!(projection["persona_card"].get("description").is_none());

        let mut missing = selected.clone();
        missing.persona_card_id = "missing".to_owned();
        missing.runtime_profile_key = "profile-missing".to_owned();
        let error = store
            .execute_agent_configuration(
                TrustedPrincipal(&principal),
                "configure-missing-persona",
                &json!({"agent_id": selected.agent_id.as_str(), "persona_card_id": "missing"}),
                &selected.runtime_profile_key,
                &missing,
            )
            .await
            .err()
            .unwrap_or_else(|| panic!("missing persona must fail"));
        assert!(matches!(
            error,
            PersistenceError::CommandRejected { code, .. } if matches!(code.as_bytes(), b"persona_not_found")
        ));
        let retained = store
            .snapshot("general", 0, 200)
            .await
            .unwrap_or_else(|error| panic!("snapshot retained persona: {error}"));
        assert_eq!(retained.agent_sessions[0].persona_card_id.as_ref(), "guide");
        let encoded = sqlx::query_scalar::<_, String>(
            "SELECT session_json FROM agent_sessions WHERE room_id = 'general' LIMIT 1",
        )
        .fetch_one(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("read selected session: {error}"));
        let mut inconsistent: serde_json::Value = serde_json::from_str(&encoded)
            .unwrap_or_else(|error| panic!("decode selected session: {error}"));
        inconsistent["persona_card"] = serde_json::Value::Null;
        assert!(serde_json::from_value::<DurableAgentSession>(inconsistent).is_err());
        let mut empty_id_with_summary: serde_json::Value = serde_json::from_str(&encoded)
            .unwrap_or_else(|error| panic!("decode selected session: {error}"));
        empty_id_with_summary["persona_card_id"] = json!("");
        empty_id_with_summary["persona_card"]["id"] = json!("");
        assert!(serde_json::from_value::<DurableAgentSession>(empty_id_with_summary).is_err());

        let mut cleared = selected.clone();
        cleared.persona_card_id.clear();
        cleared.runtime_profile_key = "profile-clear".to_owned();
        let outcome = store
            .execute_agent_configuration(
                TrustedPrincipal(&principal),
                "configure-clear-persona",
                &json!({"agent_id": selected.agent_id.as_str(), "persona_card_id": ""}),
                &selected.runtime_profile_key,
                &cleared,
            )
            .await
            .unwrap_or_else(|error| panic!("clear persona: {error}"));
        assert_eq!(outcome.result["agent_session"]["persona_card_id"], "");
        assert!(outcome.result["agent_session"]["persona_card"].is_null());
    }

    #[tokio::test]
    async fn command_result_failure_rolls_back_session_participant_and_event() {
        let (store, principal, directory) = fixture().await;
        sqlx::query(
            "CREATE TRIGGER reject_agent_result BEFORE INSERT ON command_results BEGIN SELECT RAISE(ABORT, 'injected failure'); END",
        )
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("install trigger: {error}"));
        let workspace = directory
            .path()
            .to_str()
            .unwrap_or_else(|| panic!("test workspace path must be UTF-8"));
        let result = store
            .execute_agent_create(
                TrustedPrincipal(&principal),
                "create-fails",
                &json!({"provider_id": "codex"}),
                &draft(workspace),
            )
            .await;
        assert!(result.is_err());
        let snapshot = store
            .snapshot("general", 0, 200)
            .await
            .unwrap_or_else(|error| panic!("snapshot: {error}"));
        assert!(snapshot.agent_sessions.is_empty());
        assert_eq!(snapshot.participants.len(), 1);
        assert_eq!(snapshot.events.len(), 1);
        assert_eq!(snapshot.events[0].event_type, "room_created");
    }

    #[tokio::test]
    async fn pending_lifecycle_request_blocks_agent_create() {
        let (store, principal, directory) = fixture().await;
        sqlx::query(
            "INSERT INTO lifecycle_command_reservations(room_id, principal_id, request_id, action, payload_hash, principal_json, payload_json, supervisor_generation, session_id, operation_id) VALUES ('general', ?, 'reserved-create', 'agent.start', 'reserved-hash', ?, ?, 'fixture-generation', 'existing-agent', 'reserved-operation')",
        )
        .bind(&principal.principal_id)
        .bind(serde_json::to_string(&principal).unwrap_or_else(|error| panic!("encode principal: {error}")))
        .bind(r#"{"agent_id":"existing-agent"}"#)
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("insert lifecycle reservation: {error}"));
        let workspace = directory
            .path()
            .to_str()
            .unwrap_or_else(|| panic!("test workspace path must be UTF-8"));

        assert!(matches!(
            store
                .execute_agent_create(
                    TrustedPrincipal(&principal),
                    "reserved-create",
                    &json!({"provider_id": "codex"}),
                    &draft(workspace),
                )
                .await,
            Err(PersistenceError::CommandConflict)
        ));
        let counts = sqlx::query_as::<_, (i64, i64, i64)>(
            "SELECT (SELECT COUNT(*) FROM agent_sessions), (SELECT COUNT(*) FROM room_events), (SELECT COUNT(*) FROM command_results)",
        )
        .fetch_one(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("inspect rejected create: {error}"));
        assert_eq!(counts, (0, 1, 0));
    }

    #[tokio::test]
    async fn in_place_executable_change_rejects_without_partial_authority() {
        let (store, principal, directory) = fixture().await;
        let executable = directory.path().join("provider-fixture");
        std::fs::write(&executable, b"first provider bytes")
            .unwrap_or_else(|error| panic!("write executable fixture: {error}"));
        make_executable(&executable);
        let workspace = directory
            .path()
            .to_str()
            .unwrap_or_else(|| panic!("test workspace path must be UTF-8"));
        let mut session = draft(workspace);
        (session.executable, session.executable_identity) = executable_authority(&executable);
        std::fs::write(&executable, b"changed provider bytes")
            .unwrap_or_else(|error| panic!("overwrite executable fixture: {error}"));

        let result = store
            .execute_agent_create(
                TrustedPrincipal(&principal),
                "create-changed-executable",
                &json!({"provider_id": "codex"}),
                &session,
            )
            .await;
        assert!(matches!(
            result,
            Err(PersistenceError::CommandRejected { code, .. }) if matches!(code.as_bytes(), b"runtime_authority_changed")
        ));
        let snapshot = store
            .snapshot("general", 0, 200)
            .await
            .unwrap_or_else(|error| panic!("snapshot after authority rejection: {error}"));
        assert!(snapshot.agent_sessions.is_empty());
        assert_eq!(snapshot.participants.len(), 1);
        assert_eq!(snapshot.events.len(), 1);
        assert_eq!(snapshot.events[0].event_type, "room_created");
    }

    #[tokio::test]
    async fn room_capacity_rejects_without_partial_authority() {
        let (store, principal, directory) = fixture().await;
        for index in 0..crate::sqlite::MAX_AGENT_SESSIONS_PER_ROOM {
            sqlx::query(
                "INSERT INTO agent_sessions(room_id, session_id, session_json) VALUES ('general', ?, '{}')",
            )
            .bind(format!("existing-{index}"))
            .execute(&store.pool)
            .await
            .unwrap_or_else(|error| panic!("insert capacity fixture: {error}"));
        }
        let workspace = directory
            .path()
            .to_str()
            .unwrap_or_else(|| panic!("test workspace path must be UTF-8"));
        let outcome = store
            .execute_agent_create(
                TrustedPrincipal(&principal),
                "create-capacity",
                &json!({"provider_id": "codex"}),
                &draft(workspace),
            )
            .await;
        assert!(
            matches!(
                &outcome,
                Err(PersistenceError::CommandRejected { code, .. }) if matches!(code.as_bytes(), b"agent_session_capacity")
            ),
            "unexpected capacity outcome: {outcome:?}"
        );
        let counts = sqlx::query_as::<_, (i64, i64, i64, i64)>(
            "SELECT (SELECT COUNT(*) FROM agent_sessions), (SELECT COUNT(*) FROM participants), (SELECT COUNT(*) FROM room_events), (SELECT COUNT(*) FROM command_results)",
        )
        .fetch_one(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("inspect capacity rejection: {error}"));
        assert_eq!(
            counts,
            (crate::sqlite::MAX_AGENT_SESSIONS_PER_ROOM, 1, 1, 0)
        );
    }

    #[tokio::test]
    async fn missing_agent_control_rejects_without_partial_authority() {
        let (store, mut principal, directory) = fixture().await;
        principal.capabilities.agent_control = false;
        let workspace = directory
            .path()
            .to_str()
            .unwrap_or_else(|| panic!("test workspace path must be UTF-8"));
        let mut selection = draft(workspace);
        selection.permission_mode = "full_access".into();
        let result = store
            .execute_agent_create(
                TrustedPrincipal(&principal),
                "create-denied",
                &json!({"provider_id": "codex"}),
                &selection,
            )
            .await;
        assert!(matches!(
            result,
            Err(PersistenceError::CommandRejected { code, .. }) if matches!(code.as_bytes(), b"permission_denied")
        ));
        principal.capabilities.agent_control = true;
        store
            .execute_agent_create(
                TrustedPrincipal(&principal),
                "owner-full",
                &json!({}),
                &selection,
            )
            .await
            .unwrap_or_else(|error| panic!("owner create: {error}"));
        let payload = json!({"agent_id":selection.agent_id, "permission_mode":"full_access"});
        principal.capabilities.agent_control = false;
        assert!(
            matches!(store.agent_configuration_candidate(TrustedPrincipal(&principal), &payload).await,
            Err(PersistenceError::CommandRejected { code, .. }) if code == "permission_denied")
        );
        assert!(
            matches!(store.execute_agent_configuration(TrustedPrincipal(&principal), "denied-config", &payload,
            &selection.runtime_profile_key, &selection).await,
            Err(PersistenceError::CommandRejected { code, .. }) if code == "permission_denied")
        );
        principal.capabilities.agent_control = true;
        let mut companion = store
            .agent_configuration_candidate(TrustedPrincipal(&principal), &payload)
            .await
            .unwrap_or_else(|error| panic!("candidate: {error}"));
        companion.public.external_owned = true;
        companion.public.process_ownership = "companion".into();
        persist_native_session_fixture(&store, &companion).await;
        assert!(
            matches!(store.agent_configuration_candidate(TrustedPrincipal(&principal), &payload).await,
            Err(PersistenceError::CommandRejected { code, .. }) if code == "external_runtime_owned")
        );
        assert!(
            matches!(store.execute_agent_configuration(TrustedPrincipal(&principal), "remote-config", &payload,
            &selection.runtime_profile_key, &selection).await,
            Err(PersistenceError::CommandRejected { code, .. }) if code == "external_runtime_owned")
        );
        let snapshot = store
            .snapshot("general", 0, 200)
            .await
            .unwrap_or_else(|error| panic!("snapshot: {error}"));
        assert_eq!(snapshot.agent_sessions.len(), 1);
        assert_eq!(snapshot.participants.len(), 2);
        assert_eq!(snapshot.events.len(), 2);
        assert_eq!(snapshot.events[0].event_type, "room_created");
    }

    fn persona_card(id: &str, display_name: &str) -> PersonaCard {
        PersonaCard {
            id: id.to_owned(),
            display_name: display_name.to_owned(),
            description: "private description".to_owned(),
            system_prompt: "private system prompt".to_owned(),
            personality: String::new(),
            scenario: String::new(),
            first_message: String::new(),
            example_messages: String::new(),
            post_history_instructions: String::new(),
            lorebook: Vec::new(),
            lore_settings: PersonaLoreSettings::default(),
            asset_kind: PersonaAssetKind::Card,
            source_kind: "fixture".to_owned(),
            asset_count: 0,
            ignored_features: BTreeMap::new(),
            tag_count: 0,
        }
    }
}
