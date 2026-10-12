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
    reconstruct_payload(
        &payload,
        &rebuilt,
        crate::schema_version::CURRENT_SCHEMA_VERSION,
    )
    .await?;
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
                reconstruct_payload(
                    &payload,
                    &output,
                    crate::schema_version::CURRENT_SCHEMA_VERSION
                )
                .await,
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

#[tokio::test]
async fn assets_roundtrip_exact_bytes_and_reject_corrupt_image_payload()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let store = SqliteStore::open_path(&directory.path().join("source.sqlite")).await?;
    store
        .bootstrap_local_authority(&uuid::Uuid::new_v4().to_string(), "Owner")
        .await?;
    let mut png = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgba8(3, 2).write_to(&mut png, image::ImageFormat::Png)?;
    let bytes = png.into_inner();
    sqlx::query("INSERT INTO profile_avatar_assets SELECT 'avatar',user_id,'avatar.png','image/png',?,?,?,'current',NULL FROM user_profiles")
        .bind(&bytes).bind(i64::try_from(bytes.len())?).bind(chrono::Utc::now().to_rfc3339())
        .execute(&store.pool).await?;
    let payload = directory.path().join("payload.sqlite");
    store
        .export_backup_payload(
            &ServerOwnerAuthority::LocalOperator,
            &directory.path().join("snapshot"),
            &payload,
        )
        .await?;
    let output = directory.path().join("rebuilt.sqlite");
    reconstruct_payload(
        &payload,
        &output,
        crate::schema_version::CURRENT_SCHEMA_VERSION,
    )
    .await?;
    let mut restored = open_input(&output).await?;
    let actual: Vec<u8> = sqlx::query_scalar("SELECT content FROM profile_avatar_assets")
        .fetch_one(&mut restored)
        .await?;
    assert_eq!(actual, bytes);
    restored.close().await?;
    let mut attacker =
        SqliteConnection::connect_with(&SqliteConnectOptions::new().filename(&payload)).await?;
    sqlx::query("UPDATE profile_avatar_assets SET content=zeroblob(size)")
        .execute(&mut attacker)
        .await?;
    attacker.close().await?;
    assert!(
        reconstruct_payload(
            &payload,
            &directory.path().join("rejected.sqlite"),
            crate::schema_version::CURRENT_SCHEMA_VERSION
        )
        .await
        .is_err()
    );
    Ok(())
}

// Budget the SQLite VM, not elapsed time or a pinned SQL string. Replacing the
// bounded readers with full result collection hits the budget before rejection.
#[tokio::test]
async fn hostile_schema_rejection_does_not_scan_all_objects()
-> Result<(), Box<dyn std::error::Error>> {
    let mut source = SqliteConnection::connect("sqlite::memory:").await?;
    sqlx::raw_sql("PRAGMA writable_schema=ON; CREATE TABLE seed(value TEXT);
        WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i+1 FROM n WHERE i<10000)
        INSERT INTO sqlite_schema(type,name,tbl_name,rootpage,sql)
        SELECT 'table','hostile_'||i,'hostile_'||i,0,'CREATE TABLE hostile_'||i||'(value TEXT)' FROM n;")
        .execute(&mut source).await?;
    let directory = tempfile::tempdir()?;
    let mut target = create_payload(&directory.path().join("owned.sqlite")).await?;
    let expected = objects(&mut target, None).await?;
    let budget = i32::try_from((expected.len() + 1) * 100)?;
    source
        .lock_handle()
        .await?
        .set_progress_handler(budget, || false);
    assert!(matches!(
        require_schema(&mut source, &mut target, true).await,
        Err(PersistenceError::InvalidBackup)
    ));
    source.lock_handle().await?.remove_progress_handler();
    Ok(())
}

#[tokio::test]
async fn hostile_schema_lengths_and_fk_results_are_bounded()
-> Result<(), Box<dyn std::error::Error>> {
    let mut source = SqliteConnection::connect("sqlite::memory:").await?;
    sqlx::raw_sql(
        "PRAGMA writable_schema=ON; CREATE TABLE seed(value TEXT);
        INSERT INTO sqlite_schema(type,name,tbl_name,rootpage,sql)
        VALUES('table',printf('%.*c',1000000,'x'),'seed',0,'CREATE TABLE x(value TEXT)');",
    )
    .execute(&mut source)
    .await?;
    assert!(matches!(
        objects(&mut source, Some(10)).await,
        Err(PersistenceError::InvalidBackup)
    ));
    sqlx::query("UPDATE sqlite_schema SET name='oversized_sql',sql=printf('%.*c',1000000,'x') WHERE name!='seed'").execute(&mut source).await?;
    assert!(matches!(
        objects(&mut source, Some(10)).await,
        Err(PersistenceError::InvalidBackup)
    ));
    let mut source = SqliteConnection::connect("sqlite::memory:").await?;
    sqlx::raw_sql(
        "PRAGMA foreign_keys=OFF; CREATE TABLE parent(id INTEGER PRIMARY KEY);
        CREATE TABLE child(id INTEGER REFERENCES parent(id));
        WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i+1 FROM n WHERE i<10000)
        INSERT INTO child SELECT i FROM n;",
    )
    .execute(&mut source)
    .await?;
    source
        .lock_handle()
        .await?
        .set_progress_handler(1000, || false);
    assert!(matches!(
        require_foreign_keys(&mut source).await,
        Err(PersistenceError::InvalidBackup)
    ));
    source.lock_handle().await?.remove_progress_handler();
    Ok(())
}

#[tokio::test]
async fn aggregate_rows_are_rejected_before_materialization()
-> Result<(), Box<dyn std::error::Error>> {
    let mut source = SqliteConnection::connect("sqlite::memory:").await?;
    sqlx::raw_sql("CREATE TABLE sample(a BLOB,b BLOB,c BLOB); INSERT INTO sample VALUES(zeroblob(12000000),zeroblob(12000000),zeroblob(12000000))")
        .execute(&mut source).await?;
    assert!(matches!(
        require_row_bounds(&mut source, "sample", "a,b,c").await,
        Err(PersistenceError::InvalidBackup)
    ));
    sqlx::query("UPDATE sample SET c=NULL")
        .execute(&mut source)
        .await?;
    require_row_bounds(&mut source, "sample", "a,b,c").await?;
    // Structured JSON and identifiers are bounded before deserialization, even
    // if a hostile file was created with CHECK constraints disabled.
    sqlx::raw_sql("CREATE TABLE persona_assets(persona_id TEXT,card_json TEXT); INSERT INTO persona_assets VALUES('valid',printf('%.*c',11000000,'x'))")
        .execute(&mut source).await?;
    assert!(
        require_row_bounds(&mut source, "persona_assets", "persona_id,card_json")
            .await
            .is_err()
    );
    sqlx::query("UPDATE persona_assets SET persona_id=printf('%.*c',81,'x'),card_json='{}'")
        .execute(&mut source)
        .await?;
    assert!(
        require_row_bounds(&mut source, "persona_assets", "persona_id,card_json")
            .await
            .is_err()
    );
    Ok(())
}
