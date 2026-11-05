use agentsassemble_domain::{DurableAgentSession, durable_runtime_profile_key};
use serde_json::json;

use crate::{
    RoomMutationAuthority::TrustedPrincipal,
    SqliteStore,
    agent_sessions::tests::{draft, fixture},
};

async fn stored(store: &SqliteStore) -> DurableAgentSession {
    let encoded: String = sqlx::query_scalar("SELECT session_json FROM agent_sessions")
        .fetch_one(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("read stored session: {error}"));
    serde_json::from_str(&encoded).unwrap_or_else(|error| panic!("decode session: {error}"))
}

/// Mimics a session bound before v103, when identities held the Unix device
/// number that macOS renumbers at boot. `profile_matches` keeps its profile key
/// consistent with those identities, as a real pre-v103 session's key is.
async fn make_legacy(store: &SqliteStore, profile_matches: bool) {
    let mut session = stored(store).await;
    session.workspace_identity = "identity-v1-legacy".to_owned();
    session.executable_identity = "content-identity-v1-legacy".to_owned();
    session.runtime_profile_key = if profile_matches {
        durable_runtime_profile_key(&session)
    } else {
        "provider-profile-v2-other".to_owned()
    };
    sqlx::query("UPDATE agent_sessions SET session_json = ?")
        .bind(serde_json::to_string(&session).unwrap_or_else(|error| panic!("encode: {error}")))
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("store legacy identities: {error}"));
    sqlx::query("UPDATE runtime_metadata SET value = '102' WHERE key = 'schema_version'")
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("store v102: {error}"));
}

async fn upgrade(store: &SqliteStore) {
    crate::file_identity_rebind::upgrade(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("upgrade to v103: {error}"));
    let version: String =
        sqlx::query_scalar("SELECT value FROM runtime_metadata WHERE key = 'schema_version'")
            .fetch_one(&store.pool)
            .await
            .unwrap_or_else(|error| panic!("read schema version: {error}"));
    assert_eq!(version, "103");
}

#[tokio::test]
async fn v103_rebinds_identities_whose_paths_and_profile_still_hold() {
    let (store, principal, directory) = fixture().await;
    let workspace = directory.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap_or_else(|error| panic!("create workspace: {error}"));
    let draft = draft(
        workspace
            .to_str()
            .unwrap_or_else(|| panic!("test workspace path must be UTF-8")),
    );
    let payload = json!({"provider_id": "codex", "catalog_revision": "catalog-1"});
    store
        .execute_agent_create(TrustedPrincipal(&principal), "create-1", &payload, &draft)
        .await
        .unwrap_or_else(|error| panic!("create session: {error}"));

    make_legacy(&store, true).await;
    upgrade(&store).await;
    let session = stored(&store).await;
    assert_eq!(session.workspace_identity, draft.workspace_identity);
    assert_eq!(session.executable_identity, draft.executable_identity);
    assert_eq!(
        session.runtime_profile_key,
        durable_runtime_profile_key(&session)
    );

    // A profile that no longer matches its own fields is left to fail closed.
    make_legacy(&store, false).await;
    upgrade(&store).await;
    assert_eq!(
        stored(&store).await.workspace_identity,
        "identity-v1-legacy"
    );

    // So is a workspace that moved away.
    make_legacy(&store, true).await;
    std::fs::rename(&workspace, directory.path().join("moved"))
        .unwrap_or_else(|error| panic!("move workspace: {error}"));
    upgrade(&store).await;
    let session = stored(&store).await;
    assert_eq!(session.workspace_identity, "identity-v1-legacy");
    assert_eq!(session.executable_identity, draft.executable_identity);
    assert_eq!(
        session.runtime_profile_key,
        durable_runtime_profile_key(&session)
    );
}
