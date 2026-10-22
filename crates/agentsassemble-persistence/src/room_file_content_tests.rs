use super::*;
use sqlx::Connection;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[tokio::test]
async fn pieces_preserve_bytes_at_unaligned_sizes_and_ceiling_with_atomic_failure() -> TestResult {
    let mut db = sqlx::SqliteConnection::connect("sqlite::memory:").await?;
    sqlx::query("PRAGMA foreign_keys=ON")
        .execute(&mut db)
        .await?;
    // Exercise the byte owner independently of custody, which existing product
    // tests cover; each append can commit and release the writer separately.
    for parent in [RoomFileContent::Message, RoomFileContent::Connector] {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "CREATE TABLE {}(attachment_id TEXT PRIMARY KEY,size INTEGER NOT NULL)",
            parent.parent()
        )))
        .execute(&mut db)
        .await?;
    }
    sqlx::query(MESSAGE_DDL).execute(&mut db).await?;
    sqlx::query(CONNECTOR_DDL).execute(&mut db).await?;
    for kind in [RoomFileContent::Message, RoomFileContent::Connector] {
        for size in [
            1,
            PIECE_BYTES + 17,
            agentsassemble_domain::MAX_ROOM_FILE_BYTES,
        ] {
            sqlx::query(sqlx::AssertSqlSafe(format!(
                "INSERT INTO {} VALUES('file',?)",
                kind.parent()
            )))
            .bind(i64::try_from(size).unwrap())
            .execute(&mut db)
            .await?;
            let bytes: Vec<u8> = (0..size).map(|i| u8::try_from(i % 251).unwrap()).collect();
            for (sequence, piece) in bytes.chunks(PIECE_BYTES).enumerate() {
                let mut tx = db.begin().await?;
                if sequence == 0 {
                    assert!(kind.append(&mut tx, "file", 1, piece).await.is_err());
                    if size > PIECE_BYTES {
                        assert!(kind.append(&mut tx, "file", 0, &piece[..1]).await.is_err());
                    }
                }
                kind.append(&mut tx, "file", sequence, piece).await?;
                tx.commit().await?;
                assert_eq!(kind.read(&mut db, "file", sequence).await?, piece);
            }
            kind.require_complete(&mut db).await?;
            assert_eq!(kind.collect(&mut db, "file", size).await?, bytes);
            assert!(
                kind.append(&mut db, "file", size.div_ceil(PIECE_BYTES), &[1])
                    .await
                    .is_err()
            );
            sqlx::query(sqlx::AssertSqlSafe(format!(
                "DELETE FROM {} WHERE attachment_id='file'",
                kind.parent()
            )))
            .execute(&mut db)
            .await?;
            assert_eq!(
                sqlx::query_scalar::<_, i64>(sqlx::AssertSqlSafe(format!(
                    "SELECT count(*) FROM {}",
                    kind.chunks()
                )))
                .fetch_one(&mut db)
                .await?,
                0
            );
            assert!(kind.read(&mut db, "file", 0).await.is_err());
        }
    }
    Ok(())
}
