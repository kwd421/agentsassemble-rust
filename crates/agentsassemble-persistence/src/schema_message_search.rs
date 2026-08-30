pub(crate) const RECORDS_DDL: &str = concat!(
    "CREATE TABLE IF NOT EXISTS room_message_search_records (",
    "id INTEGER PRIMARY KEY, ",
    "room_id TEXT NOT NULL, ",
    "event_seq INTEGER NOT NULL CHECK(event_seq > 0), ",
    "event_id TEXT NOT NULL CHECK(typeof(event_id) = 'text' ",
    "AND length(CAST(event_id AS BLOB)) BETWEEN 1 AND 128 ",
    "AND instr(event_id, char(0)) = 0), ",
    "created_at_nanos INTEGER NOT NULL CHECK(created_at_nanos > 0), ",
    "search_text TEXT NOT NULL CHECK(typeof(search_text) = 'text' AND length(search_text) > 0), ",
    "compact_text TEXT NOT NULL CHECK(typeof(compact_text) = 'text' AND length(compact_text) > 0), ",
    "UNIQUE(room_id, event_seq), ",
    "UNIQUE(room_id, event_id), ",
    "FOREIGN KEY(room_id, event_seq) REFERENCES room_events(room_id, seq) ON DELETE CASCADE)",
);

pub(crate) const PHRASE_DDL: &str = "CREATE VIRTUAL TABLE IF NOT EXISTS room_message_search_phrase USING fts5(search_text, content='', contentless_delete=1, tokenize='unicode61')";

pub(crate) const AUXILIARY_DDL: &[&str] = &[
    "CREATE INDEX IF NOT EXISTS room_message_search_page_idx ON room_message_search_records(room_id, created_at_nanos DESC, event_seq DESC)",
    "CREATE TRIGGER IF NOT EXISTS room_message_search_insert AFTER INSERT ON room_message_search_records BEGIN INSERT INTO room_message_search_phrase(rowid, search_text) VALUES (NEW.id, NEW.search_text); END",
    "CREATE TRIGGER IF NOT EXISTS room_message_search_delete AFTER DELETE ON room_message_search_records BEGIN DELETE FROM room_message_search_phrase WHERE rowid = OLD.id; END",
];

#[cfg(test)]
pub(crate) const FTS_SHADOW_TABLES: &[&str] = &[
    "room_message_search_phrase_config",
    "room_message_search_phrase_data",
    "room_message_search_phrase_docsize",
    "room_message_search_phrase_idx",
];

#[cfg(test)]
#[path = "schema_message_search_tests.rs"]
mod tests;
