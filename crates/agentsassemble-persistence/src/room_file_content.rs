//! The room-file byte owner. Callers retain authority, metadata and transaction
//! ownership; each operation below crosses `SQLx` with at most one 64 KiB piece.
use crate::{PersistenceError, message_attachments::invalid_attachment_state};
use sqlx::{Row, SqliteConnection};

pub(crate) const PIECE_BYTES: usize = 64 * 1024;

pub(crate) const MESSAGE_DDL: &str = "CREATE TABLE \"room_message_attachment_chunks\" (
    attachment_id TEXT NOT NULL REFERENCES \"room_message_attachments\"(attachment_id) ON DELETE CASCADE,
    sequence INTEGER NOT NULL CHECK(sequence >= 0 AND sequence < 800),
    content BLOB NOT NULL CHECK(typeof(content)='blob' AND length(content) BETWEEN 1 AND 65536),
    PRIMARY KEY(attachment_id, sequence)
) WITHOUT ROWID";

pub(crate) const CONNECTOR_DDL: &str = "CREATE TABLE \"room_connector_upload_chunks\" (
    attachment_id TEXT NOT NULL REFERENCES \"room_connector_uploads\"(attachment_id) ON DELETE CASCADE,
    sequence INTEGER NOT NULL CHECK(sequence >= 0 AND sequence < 800),
    content BLOB NOT NULL CHECK(typeof(content)='blob' AND length(content) BETWEEN 1 AND 65536),
    PRIMARY KEY(attachment_id, sequence)
) WITHOUT ROWID";

#[derive(Clone, Copy)]
pub(crate) enum RoomFileContent {
    Message,
    Connector,
}

impl RoomFileContent {
    pub(crate) fn parent(self) -> &'static str {
        match self {
            Self::Message => "room_message_attachments",
            Self::Connector => "room_connector_uploads",
        }
    }

    pub(crate) fn chunks(self) -> &'static str {
        match self {
            Self::Message => "room_message_attachment_chunks",
            Self::Connector => "room_connector_upload_chunks",
        }
    }

    /// Append exactly the next fixed-size piece (or the shorter final piece).
    /// One statement: usable in a short transaction without a network await.
    pub(crate) async fn append(
        self,
        connection: &mut SqliteConnection,
        id: &str,
        sequence: usize,
        bytes: &[u8],
    ) -> Result<(), PersistenceError> {
        if bytes.is_empty() || bytes.len() > PIECE_BYTES {
            return Err(invalid_attachment_state());
        }
        let sequence = i64::try_from(sequence).map_err(|_| invalid_attachment_state())?;
        let result = sqlx::query(sqlx::AssertSqlSafe(format!(
            "INSERT INTO {}(attachment_id,sequence,content)
             SELECT attachment_id,?,? FROM {} WHERE attachment_id=?
             AND ?=(SELECT coalesce(max(sequence)+1,0) FROM {} WHERE attachment_id=?)
             AND ?=min({PIECE_BYTES},size-?*{PIECE_BYTES})",
            self.chunks(),
            self.parent(),
            self.chunks()
        )))
        .bind(sequence)
        .bind(bytes)
        .bind(id)
        .bind(sequence)
        .bind(id)
        .bind(i64::try_from(bytes.len()).unwrap_or(i64::MAX))
        .bind(sequence)
        .execute(connection)
        .await?;
        if result.rows_affected() != 1 {
            return Err(invalid_attachment_state());
        }
        Ok(())
    }

    /// Read one piece. Absence/corruption is an error, never a successful EOF.
    /// The caller derives the number of pieces from authorized parent metadata.
    pub(crate) async fn read(
        self,
        connection: &mut SqliteConnection,
        id: &str,
        sequence: usize,
    ) -> Result<Vec<u8>, PersistenceError> {
        let sequence = i64::try_from(sequence).map_err(|_| invalid_attachment_state())?;
        let row = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT substr(chunk.content,1,{PIECE_BYTES}) AS content,
             typeof(chunk.content)='blob' AND length(chunk.content)=min({PIECE_BYTES},parent.size-?*{PIECE_BYTES}) AS valid
             FROM {} AS chunk JOIN {} AS parent USING(attachment_id)
             WHERE attachment_id=? AND sequence=?",
            self.chunks(),
            self.parent()
        )))
        .bind(sequence)
        .bind(id)
        .bind(sequence)
        .fetch_optional(connection)
        .await?
        .ok_or_else(invalid_attachment_state)?;
        if !row.try_get::<bool, _>("valid")? {
            return Err(invalid_attachment_state());
        }
        Ok(row.try_get("content")?)
    }

    /// The existing whole-file boundary may assemble pieces; streaming owners
    /// use read directly and release their connection before network awaits.
    pub(crate) async fn collect(
        self,
        connection: &mut SqliteConnection,
        id: &str,
        size: usize,
    ) -> Result<Vec<u8>, PersistenceError> {
        let mut bytes = Vec::with_capacity(size);
        for sequence in 0..size.div_ceil(PIECE_BYTES) {
            bytes.extend(self.read(connection, id, sequence).await?);
        }
        if bytes.len() != size {
            return Err(invalid_attachment_state());
        }
        Ok(bytes)
    }

    /// Completed legacy requests arrive already buffered at their boundary.
    pub(crate) async fn write(
        self,
        connection: &mut SqliteConnection,
        id: &str,
        bytes: &[u8],
    ) -> Result<(), PersistenceError> {
        for (sequence, piece) in bytes.chunks(PIECE_BYTES).enumerate() {
            self.append(connection, id, sequence, piece).await?;
        }
        Ok(())
    }

    pub(crate) async fn require_complete(
        self,
        connection: &mut SqliteConnection,
    ) -> Result<(), PersistenceError> {
        let invalid: bool = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT EXISTS(SELECT 1 FROM {} AS chunk LEFT JOIN {} AS parent USING(attachment_id) WHERE parent.attachment_id IS NULL)
             OR EXISTS(SELECT 1 FROM {} AS parent WHERE
             size!=(SELECT coalesce(sum(length(content)),0) FROM {} WHERE attachment_id=parent.attachment_id)
             OR (size+{PIECE_BYTES}-1)/{PIECE_BYTES}!=(SELECT count(*) FROM {} WHERE attachment_id=parent.attachment_id)
             OR EXISTS(SELECT 1 FROM {} WHERE attachment_id=parent.attachment_id
               AND length(content)!=min({PIECE_BYTES},size-sequence*{PIECE_BYTES})))",
            self.chunks(), self.parent(), self.parent(), self.chunks(), self.chunks(), self.chunks()
        )))
        .fetch_one(connection).await?;
        if invalid {
            return Err(invalid_attachment_state());
        }
        Ok(())
    }

    /// Backup/reconstruction use the same piece boundary, after parent metadata
    /// has crossed its typed policy. Keyset iteration retains only one file ID.
    pub(crate) async fn copy_all(
        self,
        source: &mut SqliteConnection,
        output: &mut SqliteConnection,
    ) -> Result<(), PersistenceError> {
        self.require_complete(source).await?;
        let mut previous = String::new();
        loop {
            let row = sqlx::query(sqlx::AssertSqlSafe(format!(
                "SELECT attachment_id,size FROM {} WHERE attachment_id>? ORDER BY attachment_id LIMIT 1",
                self.parent()
            )))
            .bind(&previous).fetch_optional(&mut *source).await?;
            let Some(row) = row else { break };
            let id: String = row.get("attachment_id");
            let size = usize::try_from(row.get::<i64, _>("size"))
                .map_err(|_| invalid_attachment_state())?;
            for sequence in 0..size.div_ceil(PIECE_BYTES) {
                let bytes = self.read(source, &id, sequence).await?;
                self.append(output, &id, sequence, &bytes).await?;
            }
            previous = id;
        }
        Ok(())
    }

    /// Historical archive decoding only: attach the already catalog-checked,
    /// read-only source to the existing reconstruction connection. No new driver
    /// or runtime connection; the target is private, fresh historical staging.
    pub(crate) async fn attach_archive(
        source: &mut SqliteConnection,
        output: &mut SqliteConnection,
    ) -> Result<(), PersistenceError> {
        let path: String =
            sqlx::query_scalar("SELECT file FROM pragma_database_list WHERE name='main'")
                .fetch_one(source)
                .await?;
        let mut uri = url::Url::from_file_path(path).map_err(|()| invalid_attachment_state())?;
        uri.set_query(Some("mode=ro"));
        sqlx::query("ATTACH DATABASE ? AS room_file_archive")
            .bind(uri.as_str())
            .execute(output)
            .await?;
        Ok(())
    }

    pub(crate) async fn read_archive_piece(
        connection: &mut SqliteConnection,
        id: &str,
        sequence: usize,
    ) -> Result<Vec<u8>, PersistenceError> {
        let bytes: Vec<u8> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT substr(content,?*{PIECE_BYTES}+1,{PIECE_BYTES}) FROM room_file_archive.room_message_attachments WHERE attachment_id=?"
        )))
        .bind(i64::try_from(sequence).unwrap_or(i64::MAX)).bind(id).fetch_one(connection).await?;
        Ok(bytes)
    }

    /// Keep historical content inside `SQLite` until its normal staged migration.
    /// Metadata and raster policy have already crossed archive validation. Never
    /// bind a whole old BLOB through `SQLx`, or concatenate pieces into a BLOB.
    pub(crate) async fn copy_archive_file(
        connection: &mut SqliteConnection,
        id: &str,
    ) -> Result<(), PersistenceError> {
        sqlx::query("INSERT INTO room_message_attachments(attachment_id,room_id,pending_owner_user_id,event_seq,filename,content_type,content,size,is_safe_image,created_at,state,expires_at)
            SELECT attachment_id,room_id,pending_owner_user_id,event_seq,filename,content_type,content,size,is_safe_image,created_at,state,expires_at
            FROM room_file_archive.room_message_attachments WHERE attachment_id=?")
            .bind(id).execute(connection).await?;
        Ok(())
    }

    /// Connector publication copies pieces inside `SQLite`, before parent deletion.
    pub(crate) async fn publish_connector(
        connection: &mut SqliteConnection,
        id: &str,
    ) -> Result<(), PersistenceError> {
        sqlx::query("INSERT INTO room_message_attachment_chunks(attachment_id,sequence,content)
            SELECT attachment_id,sequence,content FROM room_connector_upload_chunks WHERE attachment_id=?")
            .bind(id).execute(connection).await?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "room_file_content_tests.rs"]
mod tests;
