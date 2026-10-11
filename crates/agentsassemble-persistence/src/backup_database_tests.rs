use super::*;
use crate::{ServerOwnerAuthority, SqliteStore};

#[tokio::test]
async fn reconstruction_preserves_identity_but_never_copies_host_authority()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let store = SqliteStore::open_path(&directory.path().join("source.sqlite")).await?;
    let bootstrap = store
        .bootstrap_local_authority(&uuid::Uuid::new_v4().to_string(), "Backup owner")
        .await?;
    store
        .create_room_for_local_operator(&uuid::Uuid::new_v4().to_string(), "general", "Backup room")
        .await?;
    // Old authority bytes are deliberately planted in a retired grant owner.
    let secret = b"old-device-authority-canary-1234";
    sqlx::query("INSERT INTO central_owner_grants VALUES(?,?,'https://old.example',1,9999999999)")
        .bind(secret.as_slice())
        .bind(secret.as_slice())
        .execute(&store.pool)
        .await?;
    let snapshot = directory.path().join("snapshot.sqlite");
    let payload = directory.path().join("payload.sqlite");
    store
        .export_backup_payload(&ServerOwnerAuthority::LocalOperator, &snapshot, &payload)
        .await?;
    let rebuilt = directory.path().join("rebuilt.sqlite");
    reconstruct_payload(&payload, &rebuilt).await?;
    let mut restored = open_input(&rebuilt).await?;
    let identity: String =
        sqlx::query_scalar("SELECT value FROM runtime_metadata WHERE key='server_id'")
            .fetch_one(&mut restored)
            .await?;
    assert_eq!(identity, bootstrap.status.server_id);
    let profile: String = sqlx::query_scalar("SELECT profile_json FROM user_profiles")
        .fetch_one(&mut restored)
        .await?;
    assert!(profile.contains("Backup owner"));
    for table in crate::schema::tables()
        .filter(|t| backup_policy::policy(t.name).ok() == Some(Policy::Ephemeral))
    {
        let count: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT count(*) FROM \"{}\"",
            table.name
        )))
        .fetch_one(&mut restored)
        .await?;
        assert_eq!(
            count, 0,
            "credential or executable state survived in {}",
            table.name
        );
    }
    restored.close().await?;
    assert!(
        !std::fs::read(rebuilt)?
            .windows(secret.len())
            .any(|w| w == secret)
    );
    let source_count: i64 = sqlx::query_scalar("SELECT count(*) FROM central_owner_grants")
        .fetch_one(&store.pool)
        .await?;
    assert_eq!(
        source_count, 1,
        "snapshot sanitization changed the live host"
    );
    Ok(())
}

#[tokio::test]
async fn hostile_schema_and_unknown_json_never_become_active_authority()
-> Result<(), Box<dyn std::error::Error>> {
    for mutation in [
        "CREATE TABLE hidden_secret(value TEXT)",
        "CREATE VIEW hidden_authority AS SELECT * FROM user_profiles",
        "CREATE TRIGGER hidden_effect AFTER INSERT ON rooms BEGIN DELETE FROM user_profiles; END",
        "ALTER TABLE user_profiles ADD COLUMN secret TEXT",
        "UPDATE user_profiles SET profile_json=json_set(profile_json,'$.unrecognized',1)",
        "UPDATE user_profiles SET profile_json=json_set(profile_json,'$.display_name',printf('%.*c',20*1024*1024,'x'))",
    ] {
        let directory = tempfile::tempdir()?;
        let store = SqliteStore::open_path(&directory.path().join("source.sqlite")).await?;
        store
            .bootstrap_local_authority(&uuid::Uuid::new_v4().to_string(), "Owner")
            .await?;
        let payload = directory.path().join("payload.sqlite");
        store
            .export_backup_payload(
                &ServerOwnerAuthority::LocalOperator,
                &directory.path().join("snapshot"),
                &payload,
            )
            .await?;
        let mut attacker =
            SqliteConnection::connect_with(&SqliteConnectOptions::new().filename(&payload)).await?;
        sqlx::raw_sql(sqlx::AssertSqlSafe(mutation))
            .execute(&mut attacker)
            .await?;
        attacker.close().await?;
        let output = directory.path().join("reconstructed.sqlite");
        assert!(
            matches!(
                reconstruct_payload(&payload, &output).await,
                Err(PersistenceError::InvalidBackup)
            ),
            "accepted {mutation}"
        );
        let mut checked = open_input(&output).await?;
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM user_profiles")
            .fetch_one(&mut checked)
            .await?;
        assert_eq!(
            count, 0,
            "failed reconstruction partially committed product data"
        );
        checked.close().await?;
    }
    Ok(())
}
