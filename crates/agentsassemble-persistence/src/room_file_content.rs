//! The room-file byte owner. Callers retain authority, metadata and transaction
//! ownership; each operation below crosses SQLx with at most one 64 KiB piece.
use crate::{PersistenceError, message_attachments::invalid_attachment_state};
use sqlx::{Row, SqliteConnection};

pub(crate) const PIECE_BYTES: usize = 64 * 1024;

pub(crate) const MESSAGE_DDL: &str = "CREATE TABLE room_message_attachment_chunks (
    attachment_id TEXT NOT NULL REFERENCES room_message_attachments(attachment_id) ON DELETE CASCADE,
    sequence INTEGER NOT NULL CHECK(sequence >= 0 AND sequence < 800),
    content BLOB NOT NULL CHECK(typeof(content)='blob' AND length(content) BETWEEN 1 AND 65536),
    PRIMARY KEY(attachment_id, sequence)
) WITHOUT ROWID";

pub(crate) const CONNECTOR_DDL: &str = "CREATE TABLE room_connector_upload_chunks (
    attachment_id TEXT NOT NULL REFERENCES room_connector_uploads(attachment_id) ON DELETE CASCADE,
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
        .bind(bytes.len() as i64)
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
             length(chunk.content)=min({PIECE_BYTES},parent.size-?*{PIECE_BYTES}) AS valid
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
        if !row.get::<bool, _>("valid") {
            return Err(invalid_attachment_state());
        }
        Ok(row.get("content"))
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
            "SELECT EXISTS(SELECT 1 FROM {} AS parent WHERE
             size!=(SELECT coalesce(sum(length(content)),0) FROM {} WHERE attachment_id=parent.attachment_id)
             OR (size+{PIECE_BYTES}-1)/{PIECE_BYTES}!=(SELECT count(*) FROM {} WHERE attachment_id=parent.attachment_id)
             OR EXISTS(SELECT 1 FROM {} WHERE attachment_id=parent.attachment_id
               AND length(content)!=min({PIECE_BYTES},size-sequence*{PIECE_BYTES})))",
            self.parent(), self.chunks(), self.chunks(), self.chunks()
        )))
        .fetch_one(connection).await?;
        if invalid {
            return Err(invalid_attachment_state());
        }
        Ok(())
    }

    /// Connector publication copies pieces inside SQLite, before parent deletion.
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
