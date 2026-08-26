use super::tests::{
    SessionAuthority, insert_human_session, installed_schema, seed_human_authorities,
};

#[tokio::test]
async fn reusable_invite_accepts_distinct_presented_credential_admissions() {
    let pool = installed_schema().await;
    seed_human_authorities(&pool).await;

    insert_human_session(
        &pool,
        SessionAuthority {
            marker: 9,
            invite_id: "2222222222222222",
            key_kind: "reusable",
            room_id: "room-a",
            user_id: "user-a",
            participant_id: "participant-a",
            invite_scope: "read_write",
            reusable_identity: Some(vec![0x71; 32]),
        },
    )
    .await
    .unwrap_or_else(|error| panic!("insert signed-token admission: {error}"));
    sqlx::query(concat!(
        "INSERT INTO human_room_sessions(admission_key, key_kind, first_request_id, invite_id, ",
        "payload_hash, session_fingerprint, room_id, user_id, participant_id, client_kind, invite_scope, browser_credential_fingerprint, reusable_identity_fingerprint, result_json, admitted_at, expires_at, state) ",
        "SELECT ?, key_kind, 'bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb', invite_id, payload_hash, ?, room_id, user_id, participant_id, client_kind, invite_scope, browser_credential_fingerprint, reusable_identity_fingerprint, result_json, admitted_at, expires_at, state ",
        "FROM human_room_sessions WHERE admission_key = ?",
    ))
    .bind(vec![0x0A; 32])
    .bind(vec![0x4A; 32])
    .bind(vec![9; 32])
    .execute(&pool)
    .await
    .unwrap_or_else(|error| panic!("insert join-code admission: {error}"));

    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM human_room_sessions WHERE invite_id = ? AND reusable_identity_fingerprint = ?",
        )
        .bind("2222222222222222")
        .bind(vec![0x71; 32])
        .fetch_one(&pool)
        .await
        .unwrap_or_else(|error| panic!("count distinct reusable admissions: {error}")),
        2
    );
}
