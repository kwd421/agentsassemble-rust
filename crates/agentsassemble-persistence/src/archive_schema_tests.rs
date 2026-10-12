use super::*;
use crate::{
    ServerOwnerAuthority, SqliteStore,
    backup_codec::{self, BackupManifest},
};

// Build source-version product data in source-version trusted DDL. No database
// binary or downgraded current schema is used as an historical archive.
pub(crate) async fn archive(
    directory: &Path,
    version: i64,
) -> Result<(std::path::PathBuf, BackupManifest), Box<dyn std::error::Error>> {
    let seed = product_seed(directory, version).await?;
    let path = directory.join("historical.sqlite");
    let mut target = create_version_payload(&path, version).await?;
    sqlx::query("ATTACH DATABASE ? AS seed")
        .bind(seed.to_str().ok_or("path")?)
        .execute(&mut target)
        .await?;
    for table in crate::archive_schema::tables(&mut target).await? {
        if backup_policy::policy(&table)? == Policy::Ephemeral {
            continue;
        }
        let fields = columns(&mut target, &table)
            .await?
            .into_iter()
            .filter(|c| c.2 == 0)
            .map(|c| format!("\"{}\"", c.0))
            .collect::<Vec<_>>()
            .join(",");
        // Historical fixtures contain one small piece; use the actual old
        // table DDL and named columns, not a relabeled current database.
        let selected = if version < 93 && table == "room_message_attachments" {
            fields.replace("\"content\"", "(SELECT content FROM seed.room_message_attachment_chunks WHERE attachment_id=seed.room_message_attachments.attachment_id AND sequence=0) AS content")
        } else {
            fields.clone()
        };
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "INSERT INTO main.\"{table}\"({fields}) SELECT {selected} FROM seed.\"{table}\""
        )))
        .execute(&mut target)
        .await?;
    }
    sqlx::query("UPDATE runtime_metadata SET value=? WHERE key='schema_version'")
        .bind(version.to_string())
        .execute(&mut target)
        .await?;
    sqlx::query("DETACH DATABASE seed")
        .execute(&mut target)
        .await?;
    let server_id = sqlx::query_scalar("SELECT value FROM runtime_metadata WHERE key='server_id'")
        .fetch_one(&mut target)
        .await?;
    let (rooms, messages, assets) = crate::restore_preparation::counts(&mut target).await?;
    target.close().await?;
    let (database_bytes, database_sha256) =
        backup_codec::measure_payload(&mut std::fs::File::open(&path)?)?;
    let manifest = BackupManifest {
        schema_version: version,
        server_id,
        server_name: "Historical owner".into(),
        issuer: "https://central.example".into(),
        owner_person_id: "owner".into(),
        source_epoch: "epoch".into(),
        source_key_fingerprint: "a".repeat(43),
        snapshot_at: chrono::Utc::now(),
        rooms,
        messages,
        assets,
        database_bytes,
        database_sha256,
    };
    let mut encoded = std::io::Cursor::new(Vec::new());
    backup_codec::encode(
        &mut std::fs::File::open(&path)?,
        &mut encoded,
        &manifest,
        None,
        Default::default(),
    )?;
    encoded.set_position(0);
    let decoded = directory.join("decoded.sqlite");
    let manifest = backup_codec::decode(&mut encoded, &mut std::fs::File::create(&decoded)?, None)?;
    Ok((decoded, manifest))
}

async fn product_seed(
    directory: &Path,
    version: i64,
) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    let store = SqliteStore::open_path(&directory.join("live.sqlite")).await?;
    store
        .bootstrap_local_authority(&uuid::Uuid::new_v4().to_string(), "Historical owner")
        .await?;
    store
        .create_room_for_local_operator(
            &uuid::Uuid::new_v4().to_string(),
            "general",
            "Historical room",
        )
        .await?;
    if version == 87 {
        sqlx::query("UPDATE rooms SET settings_json=json_remove(settings_json,'$.lobby_name')")
            .execute(&store.pool)
            .await?;
        // Creation receipts/events also contain settings: use the same historical
        // representation, not current-only defaults, in the fixture seed.
        for (table, column) in [
            ("room_events", "event_json"),
            ("room_create_results", "result_json"),
        ] {
            sqlx::query(sqlx::AssertSqlSafe(format!("UPDATE {table} SET {column}=json_remove({column},'$.settings.lobby_name','$.room_settings.lobby_name')"))).execute(&store.pool).await?;
        }
    }
    let bytes = b"historical attachment\0\xff\x01";
    sqlx::query("INSERT INTO room_message_attachments(attachment_id,room_id,pending_owner_user_id,filename,content_type,size,is_safe_image,created_at,state,expires_at) SELECT ?, 'general',user_id,'history.bin','application/octet-stream',?,0,1,'pending',9223372036854775807 FROM user_profiles LIMIT 1")
        .bind(format!("ma_{}", "a".repeat(32))).bind(i64::try_from(bytes.len())?).execute(&store.pool).await?;
    crate::room_file_content::RoomFileContent::Message
        .write(
            &mut *store.pool.acquire().await?,
            &format!("ma_{}", "a".repeat(32)),
            bytes,
        )
        .await?;
    let principal = agentsassemble_domain::AuthenticatedPrincipal {
        principal_id: agentsassemble_domain::LOCAL_OPERATOR_USER_ID.into(),
        participant_id: agentsassemble_domain::LOCAL_OPERATOR_PARTICIPANT_ID.into(),
        display_name: "Historical owner".into(),
        room_id: "general".into(),
        client_kind: agentsassemble_domain::ClientKind::Browser,
        invite_scope: agentsassemble_domain::InviteScope::ReadWrite,
        is_operator: true,
        capabilities: agentsassemble_domain::CapabilitySet::local_operator(
            agentsassemble_domain::ClientKind::Browser,
            agentsassemble_domain::InviteScope::ReadWrite,
        ),
    };
    // Include a real pending raster in every genuine historical catalog fixture:
    // old archives validate through piece reads, new archives through chunk rows.
    let mut png = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgba8(2, 2).write_to(&mut png, image::ImageFormat::Png)?;
    store
        .store_message_attachment(&principal, "pending.png", "image/png", png.into_inner())
        .await?;
    store.execute_message(&principal, &uuid::Uuid::new_v4().to_string(), "message.send",
        &serde_json::json!({"content":"Historical searchable message", "attachment_ids":[format!("ma_{}", "a".repeat(32))]})).await?;
    let seed = directory.join("seed.sqlite");
    store
        .export_backup_payload(
            &ServerOwnerAuthority::LocalOperator,
            &directory.join("snapshot.sqlite"),
            &seed,
        )
        .await?;
    store.close().await?;
    if version < 92 {
        let mut historical =
            SqliteConnection::connect_with(&SqliteConnectOptions::new().filename(&seed)).await?;
        sqlx::query("UPDATE rooms SET settings_json=json_remove(settings_json,'$.attachment_file_limit_bytes','$.attachment_storage_limit_bytes')").execute(&mut historical).await?;
        for (table, column) in [
            ("room_events", "event_json"),
            ("room_create_results", "result_json"),
        ] {
            sqlx::query(sqlx::AssertSqlSafe(format!("UPDATE {table} SET {column}=json_remove({column},'$.settings.attachment_file_limit_bytes','$.settings.attachment_storage_limit_bytes','$.room_settings.attachment_file_limit_bytes','$.room_settings.attachment_storage_limit_bytes')"))).execute(&mut historical).await?;
        }
        historical.close().await?;
    }
    Ok(seed)
}

pub(crate) async fn assert_product_rows(
    source: &Path,
    target: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut source = open_input(source).await?;
    let mut target = open_input(target).await?;
    for table in crate::archive_schema::tables(&mut source).await? {
        if backup_policy::policy(&table)? != Policy::Product {
            continue;
        }
        let fields = columns(&mut source, &table)
            .await?
            .into_iter()
            .filter(|c| c.2 == 0)
            .map(|c| format!("\"{}\"", c.0))
            .collect::<Vec<_>>()
            .join(",");
        let query = format!("SELECT {fields} FROM \"{table}\" ORDER BY {fields}");
        let before = sqlx::query(sqlx::AssertSqlSafe(query.as_str()))
            .fetch_all(&mut source)
            .await?;
        let chunked = table == "room_message_attachments"
            && crate::archive_schema::version(&mut source).await? < 93
            && crate::archive_schema::version(&mut target).await? >= 93;
        let target_query = if chunked {
            query.replace("\"content\",", "")
        } else {
            query.clone()
        };
        let after = sqlx::query(sqlx::AssertSqlSafe(target_query.as_str()))
            .fetch_all(&mut target)
            .await?;
        assert_eq!(before.len(), after.len(), "{table} count");
        for (before, after) in before.iter().zip(&after) {
            let mut before = row_values(before)?;
            let after = row_values(after)?;
            if chunked {
                let bytes = before.remove("content").ok_or("content")?;
                let id = before
                    .get("attachment_id")
                    .and_then(Cell::text)
                    .ok_or("id")?;
                let size = match before.get("size") {
                    Some(Cell::Integer(size)) => *size as usize,
                    _ => return Err("size".into()),
                };
                assert_eq!(
                    bytes,
                    Cell::Blob(
                        crate::room_file_content::RoomFileContent::Message
                            .collect(&mut target, id, size)
                            .await?
                    )
                );
            }
            assert_eq!(before, after, "{table}");
        }
    }
    source.close().await?;
    target.close().await?;
    Ok(())
}

#[tokio::test]
async fn every_catalog_version_reconstructs_real_encoded_rows()
-> Result<(), Box<dyn std::error::Error>> {
    for version in crate::archive_schema::FLOOR..=crate::schema_version::CURRENT_SCHEMA_VERSION {
        let directory = tempfile::tempdir()?;
        let (source, _) = archive(directory.path(), version).await?;
        let target = directory.path().join("rebuilt.sqlite");
        reconstruct_payload(&source, &target, version).await?;
        assert_product_rows(&source, &target).await?;
    }
    Ok(())
}

#[tokio::test]
async fn fresh_exports_must_match_frozen_catalog() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let mut current = create_payload(&directory.path().join("current.sqlite")).await?;
    let mut catalog = create_version_payload(
        &directory.path().join("catalog.sqlite"),
        crate::schema_version::CURRENT_SCHEMA_VERSION,
    )
    .await?;
    require_schema(&mut current, &mut catalog, true).await?;
    current.close().await?;
    catalog.close().await?;
    Ok(())
}

#[tokio::test]
async fn historical_tampering_is_rejected_before_prepared() -> Result<(), Box<dyn std::error::Error>>
{
    for mutation in [
        "UPDATE runtime_metadata SET value='90' WHERE key='schema_version'",
        "UPDATE runtime_metadata SET value='86' WHERE key='schema_version'",
        "UPDATE runtime_metadata SET value='088' WHERE key='schema_version'",
        "UPDATE runtime_metadata SET value='87' WHERE key='schema_version'",
        "ALTER TABLE user_profiles ADD COLUMN secret TEXT",
        "CREATE TABLE surprise(value TEXT)",
        "DROP INDEX room_message_attachments_pending_expiry_idx; CREATE INDEX room_message_attachments_pending_expiry_idx ON room_message_attachments(size)",
        "INSERT INTO central_owner_grants VALUES(zeroblob(32),zeroblob(32),'https://archive.example',1,9999999999)",
        "UPDATE user_profiles SET profile_json=json_set(profile_json,'$.unknown',1)",
        "UPDATE room_message_attachments SET content=x'ffd80000',size=4 WHERE is_safe_image=1",
        "UPDATE rooms SET settings_json=json_set(settings_json,'$.conversation_mode','free_access')",
        "UPDATE room_create_results SET result_json=json_object('event',json_set((SELECT event_json FROM room_events ORDER BY seq LIMIT 1),'$.settings',json((SELECT settings_json FROM rooms LIMIT 1)),'$.settings.conversation_mode','free_access'))",
    ] {
        let directory = tempfile::tempdir()?;
        let (source, mut manifest) = archive(directory.path(), 88).await?;
        let mut connection =
            SqliteConnection::connect_with(&SqliteConnectOptions::new().filename(&source)).await?;
        sqlx::raw_sql(sqlx::AssertSqlSafe(mutation))
            .execute(&mut connection)
            .await?;
        connection.close().await?;
        (manifest.database_bytes, manifest.database_sha256) =
            backup_codec::measure_payload(&mut std::fs::File::open(&source)?)?;
        let destination = tempfile::tempdir()?;
        let target = destination.path().join("runtime.sqlite3");
        let mut job = crate::restore_job::RestoreJob::begin(&target)?;
        assert!(job.prepare(&source, manifest).await.is_err(), "{mutation}");
        assert_eq!(
            job.journal().phase,
            crate::restore_job::RestorePhase::Validating
        );
        assert!(job.bind_switch("epoch", &"a".repeat(43)).is_err());
        assert!(!target.exists());
        job.cancel()?;
    }
    Ok(())
}

#[tokio::test]
async fn newer_fields_cannot_enter_historical_archives() -> Result<(), Box<dyn std::error::Error>> {
    for version in 87..=91 {
        let directory = tempfile::tempdir()?;
        let (source, manifest) = archive(directory.path(), version).await?;
        for (table, column, path, value, introduced) in [
            (
                "rooms",
                "settings_json",
                "$.attachment_file_limit_bytes",
                "52428800",
                92,
            ),
            (
                "rooms",
                "settings_json",
                "$.attachment_storage_limit_bytes",
                "null",
                92,
            ),
            (
                "room_events",
                "event_json",
                "$.settings.attachment_file_limit_bytes",
                "52428800",
                92,
            ),
            (
                "room_create_results",
                "result_json",
                "$.event.settings.attachment_storage_limit_bytes",
                "null",
                92,
            ),
            (
                "room_create_results",
                "result_json",
                "$.start.events[0].settings.attachment_file_limit_bytes",
                "52428800",
                92,
            ),
            (
                "rooms",
                "settings_json",
                "$.conversation_mode",
                "\"free_access\"",
                89,
            ),
            (
                "room_events",
                "event_json",
                "$.on_behalf_of_participant_id",
                "\"owner\"",
                90,
            ),
        ] {
            if version >= introduced {
                continue;
            }
            let altered = directory
                .path()
                .join(format!("altered-{table}-{}.sqlite", path.replace('.', "_")));
            std::fs::copy(&source, &altered)?;
            let mut connection =
                SqliteConnection::connect_with(&SqliteConnectOptions::new().filename(&altered))
                    .await?;
            // Use complete current-decodable settings/events so rejection cannot
            // be explained by an unrelated malformed nested object.
            if table == "room_events" && path.starts_with("$.settings.") {
                sqlx::query("UPDATE room_events SET event_json=json_set(event_json,'$.settings',json((SELECT settings_json FROM rooms LIMIT 1)))")
                    .execute(&mut connection).await?;
            }
            if table == "room_create_results" {
                let event: String = sqlx::query_scalar("SELECT json_set(event_json,'$.settings',json((SELECT settings_json FROM rooms LIMIT 1))) FROM room_events ORDER BY seq LIMIT 1")
                    .fetch_one(&mut connection).await?;
                let event: serde_json::Value = serde_json::from_str(&event)?;
                let receipt = if path.starts_with("$.start.") {
                    serde_json::json!({"start":{"events":[event]}})
                } else {
                    serde_json::json!({"event":event})
                };
                sqlx::query("UPDATE room_create_results SET result_json=?")
                    .bind(receipt.to_string())
                    .execute(&mut connection)
                    .await?;
            }
            sqlx::query(sqlx::AssertSqlSafe(format!(
                "UPDATE {table} SET {column}=json_set({column},?,json(?))"
            )))
            .bind(path)
            .bind(value)
            .execute(&mut connection)
            .await?;
            let encoded: String = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
                "SELECT {column} FROM {table} LIMIT 1"
            )))
            .fetch_one(&mut connection)
            .await?;
            crate::backup_policy::json(table, column, &encoded, 92)?;
            connection.close().await?;
            let mut manifest = manifest.clone();
            (manifest.database_bytes, manifest.database_sha256) =
                backup_codec::measure_payload(&mut std::fs::File::open(&altered)?)?;
            let destination = tempfile::tempdir()?;
            let target = destination.path().join("runtime.sqlite3");
            let mut job = crate::restore_job::RestoreJob::begin(&target)?;
            assert!(
                job.prepare(&altered, manifest).await.is_err(),
                "v{version} {path}"
            );
            assert_eq!(
                job.journal().phase,
                crate::restore_job::RestorePhase::Validating
            );
            assert!(!target.exists());
            job.cancel()?;
        }
    }
    Ok(())
}
