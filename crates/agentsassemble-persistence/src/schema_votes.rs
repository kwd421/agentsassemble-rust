use crate::schema::TableDefinition;

pub(crate) const TABLES: &[TableDefinition] = &[
    TableDefinition {
        name: "room_vote_states",
        ddl: concat!(
            "CREATE TABLE IF NOT EXISTS room_vote_states (",
            "room_id TEXT NOT NULL, ",
            "vote_id TEXT NOT NULL CHECK(typeof(vote_id) = 'text' ",
            "AND length(CAST(vote_id AS BLOB)) BETWEEN 1 AND 128 ",
            "AND instr(vote_id, char(0)) = 0), ",
            "poll_seq INTEGER NOT NULL CHECK(poll_seq > 0), ",
            "tallies_json TEXT NOT NULL CHECK(typeof(tallies_json) = 'text' ",
            "AND length(CAST(tallies_json AS BLOB)) BETWEEN 3 AND 256 ",
            "AND json_valid(tallies_json) AND json_type(tallies_json) = 'array'), ",
            "total_votes INTEGER NOT NULL CHECK(total_votes >= 0), ",
            "manual_close_seq INTEGER ",
            "CHECK(manual_close_seq IS NULL OR manual_close_seq > poll_seq), ",
            "PRIMARY KEY(room_id, vote_id), ",
            "UNIQUE(room_id, poll_seq), ",
            "FOREIGN KEY(room_id, poll_seq) ",
            "REFERENCES room_events(room_id, seq) ON DELETE CASCADE, ",
            "FOREIGN KEY(room_id, manual_close_seq) ",
            "REFERENCES room_events(room_id, seq) ON DELETE CASCADE)",
        ),
        infrastructure: false,
    },
    TableDefinition {
        name: "room_vote_ballots",
        ddl: concat!(
            "CREATE TABLE IF NOT EXISTS room_vote_ballots (",
            "room_id TEXT NOT NULL, ",
            "vote_id TEXT NOT NULL, ",
            "participant_id TEXT NOT NULL, ",
            "choice_index INTEGER NOT NULL CHECK(choice_index BETWEEN 0 AND 9), ",
            "PRIMARY KEY(room_id, vote_id, participant_id), ",
            "FOREIGN KEY(room_id, vote_id) ",
            "REFERENCES room_vote_states(room_id, vote_id) ON DELETE CASCADE, ",
            "FOREIGN KEY(room_id, participant_id) ",
            "REFERENCES participants(room_id, participant_id))",
        ),
        infrastructure: false,
    },
];

#[cfg(test)]
mod tests {
    use crate::schema::tests::installed_schema;

    #[tokio::test]
    async fn vote_projection_is_bound_to_its_event_and_participant() {
        let pool = installed_schema().await;
        sqlx::query(
            "INSERT INTO rooms(room_id, room_json, settings_json) VALUES ('general', '{}', '{}')",
        )
        .execute(&pool)
        .await
        .unwrap_or_else(|error| panic!("insert room: {error}"));
        sqlx::query("INSERT INTO participants(room_id, participant_id, participant_json) VALUES ('general', 'human', '{}')")
            .execute(&pool)
            .await
            .unwrap_or_else(|error| panic!("insert participant: {error}"));
        sqlx::query(
            "INSERT INTO room_events(room_id, seq, event_json) VALUES ('general', 1, '{}')",
        )
        .execute(&pool)
        .await
        .unwrap_or_else(|error| panic!("insert event: {error}"));
        sqlx::query("INSERT INTO room_vote_states(room_id, vote_id, poll_seq, tallies_json, total_votes) VALUES ('general', 'vote-1', 1, '[0,0]', 0)")
            .execute(&pool)
            .await
            .unwrap_or_else(|error| panic!("insert vote: {error}"));
        sqlx::query("INSERT INTO room_vote_ballots(room_id, vote_id, participant_id, choice_index) VALUES ('general', 'vote-1', 'human', 1)")
            .execute(&pool)
            .await
            .unwrap_or_else(|error| panic!("insert ballot: {error}"));

        assert!(
            sqlx::query("INSERT INTO room_vote_ballots(room_id, vote_id, participant_id, choice_index) VALUES ('general', 'vote-1', 'missing', 0)")
                .execute(&pool)
                .await
                .is_err()
        );
        assert!(
            sqlx::query("UPDATE room_vote_ballots SET choice_index = 10 WHERE room_id = 'general'")
                .execute(&pool)
                .await
                .is_err()
        );
        assert!(
            sqlx::query(
                "UPDATE room_vote_states SET manual_close_seq = 2 WHERE room_id = 'general'"
            )
            .execute(&pool)
            .await
            .is_err()
        );

        sqlx::query("DELETE FROM room_events WHERE room_id = 'general' AND seq = 1")
            .execute(&pool)
            .await
            .unwrap_or_else(|error| panic!("delete event: {error}"));
        let ballot_count = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM room_vote_ballots")
            .fetch_one(&pool)
            .await
            .unwrap_or_else(|error| panic!("count ballots: {error}"));
        assert_eq!(ballot_count, 0);
    }
}
