use super::*;
use agentsassemble_domain::{Participant, RoomEvent};
use serde_json::{Value, json};

async fn snapshot_fixture()
-> Result<(SqliteStore, MemberRemovalKey, RoomEvent), Box<dyn std::error::Error>> {
    let (store, now) = fixture().await;
    store.set_registration_epoch(Some("epoch")).await?;
    insert_invite(&store, [1; 32], [2; 32], "guest", 10, now).await;
    let request = prepared(
        [2; 32],
        [3; 32],
        &uuid::Uuid::new_v4().to_string(),
        "ignored",
    )
    .with_member(member());
    let joined = admitted(store.admit_human(&request, now).await?);
    let participant: Participant = serde_json::from_str(
        &sqlx::query_scalar::<_, String>(
            "SELECT participant_json FROM participants WHERE participant_id=?",
        )
        .bind(&joined.result().agent_id)
        .fetch_one(&store.pool)
        .await?,
    )?;
    let mut tx = store.pool.begin().await?;
    let mut event = crate::participant_leave::participant_left_event(&mut tx, &participant).await?;
    event.event_type = "message_final".into();
    event.content = Some("kept-message ".repeat(20000));
    event.message_kind = Some("message".into());
    event
        .extra
        .insert("avatar_image_url".into(), json!("attachment:private-photo"));
    crate::room_turns::support::insert_event(&mut tx, &event).await?;
    tx.commit().await?;
    sqlx::query(
        "INSERT INTO room_message_pins(room_id,event_id,event_seq,pinned_at) VALUES (?,?,?,1)",
    )
    .bind(&event.room_id)
    .bind(&event.id)
    .bind(event.seq)
    .execute(&store.pool)
    .await?;
    let mut other = event.clone();
    other.actor.participant_id = "independent".into();
    other.participant_id = Some("independent".into());
    other.actor_id = Some("independent".into());
    let result = json!({"event":event,"events":[event,other],"result":{"agent_id":participant.participant_id,"display_name":"Private Name","avatar_image_url":"attachment:private-photo","guide":{"system_prompt":"Private Name"}},"content":"Private Name"});
    sqlx::query("INSERT INTO command_results(room_id,principal_id,request_id,action,payload_hash,result_json) VALUES (?,'other-principal','cached','message.send','hash',?)")
        .bind(&event.room_id).bind(result.to_string()).execute(&store.pool).await?;
    sqlx::query("INSERT INTO profile_avatar_assets(attachment_id,owner_user_id,filename,content_type,content,size,created_at,state) SELECT 'photo',user_id,'private.png','image/png',zeroblob(10485760),10485760,?,'current' FROM user_profiles WHERE participant_id=?")
        .bind(now.to_rfc3339()).bind(&participant.participant_id).execute(&store.pool).await?;
    let key = store
        .begin_member_account_removal(&principal("epoch")?, &secure())
        .await?;
    for _ in 0..3 {
        store.advance_member_removal_authority(&key).await?;
    }
    store.advance_member_removal_companions(&key).await?;
    Ok((store, key, event))
}

async fn scrub_until_rooms(store: &SqliteStore, key: &MemberRemovalKey) -> TestResult {
    for _ in 0..100 {
        if store.member_removal_phase(key).await? == "rooms" {
            return Ok(());
        }
        store.advance_member_removal_snapshots(key).await?;
    }
    Err("snapshot work did not finish".into())
}

#[tokio::test]
async fn exact_nested_snapshots_avatar_and_fts_preserve_content_and_independent_author()
-> TestResult {
    let (store, key, event) = snapshot_fixture().await?;
    scrub_until_rooms(&store, &key).await?;
    let raw: String =
        sqlx::query_scalar("SELECT event_json FROM room_events WHERE room_id=? AND seq=?")
            .bind(&event.room_id)
            .bind(event.seq)
            .fetch_one(&store.pool)
            .await?;
    let stored: RoomEvent = serde_json::from_str(&raw)?;
    assert_eq!(
        stored.display_name.as_deref(),
        Some(crate::DEPARTED_USER_NAME)
    );
    assert_eq!(stored.extra["avatar_image_url"], "");
    assert_eq!(stored.content, event.content);
    let cached: Value = serde_json::from_str(
        &sqlx::query_scalar::<_, String>(
            "SELECT result_json FROM command_results WHERE request_id='cached'",
        )
        .fetch_one(&store.pool)
        .await?,
    )?;
    assert_eq!(
        cached["events"][0]["display_name"],
        crate::DEPARTED_USER_NAME
    );
    assert_eq!(cached["events"][1]["display_name"], "Private Name");
    assert_eq!(
        cached["events"][1]["avatar_image_url"],
        "attachment:private-photo"
    );
    assert_eq!(cached["result"]["guide"], json!({}));
    assert_eq!(cached["content"], "Private Name");
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM profile_avatar_assets")
            .fetch_one(&store.pool)
            .await?,
        0
    );
    let profile:Value=serde_json::from_str(&sqlx::query_scalar::<_,String>("SELECT p.profile_json FROM user_profiles p JOIN central_identity_bindings b USING(user_id)").fetch_one(&store.pool).await?)?;
    assert_eq!(profile["display_name"], crate::DEPARTED_USER_NAME);
    assert_eq!(profile["avatar_image_url"], "");
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM room_message_pins")
            .fetch_one(&store.pool)
            .await?,
        1
    );
    let indexed: String =
        sqlx::query_scalar("SELECT search_text FROM room_message_search_records WHERE event_id=?")
            .bind(&event.id)
            .fetch_one(&store.pool)
            .await?;
    assert!(indexed.starts_with("탈퇴한 사용자\nkept-message"));
    assert!(!indexed.contains("private name"));
    assert_eq!(sqlx::query_scalar::<_,i64>("SELECT COUNT(*) FROM room_message_search_phrase WHERE room_message_search_phrase MATCH 'private'").fetch_one(&store.pool).await?,0);
    assert_eq!(sqlx::query_scalar::<_,i64>("SELECT COUNT(*) FROM room_message_search_phrase WHERE room_message_search_phrase MATCH 'kept'").fetch_one(&store.pool).await?,1);
    let page = store
        .advance_member_removal_room(&key)
        .await?
        .ok_or("missing room handoff")?;
    let again = store
        .advance_member_removal_room(&key)
        .await?
        .ok_or("lost handoff")?;
    assert_eq!(page.event_seq, again.event_seq);
    assert!(store.finish_member_removal_room(&key, &page).await.is_err());
    for pending in store.pending_room_publications(&page.room_id).await? {
        store
            .acknowledge_room_publication(&page.room_id, pending.seq)
            .await?;
    }
    store.finish_member_removal_room(&key, &page).await?;
    assert!(store.advance_member_removal_room(&key).await?.is_none());
    assert_eq!(sqlx::query_scalar::<_,i64>("SELECT COUNT(*) FROM room_events WHERE json_extract(event_json,'$.type')='participant_anonymized'").fetch_one(&store.pool).await?,1);
    Ok(())
}

#[tokio::test]
async fn snapshot_patch_and_cursor_rollback_together_without_exporting_large_replay() -> TestResult
{
    let (store, key, event) = snapshot_fixture().await?;
    while store.member_removal_phase(&key).await? != "room_events" {
        store.advance_member_removal_snapshots(&key).await?;
    }
    // Advance the earlier join event; the next row is the >128KiB message.
    store.advance_member_removal_snapshots(&key).await?;
    let cursor: String = sqlx::query_scalar("SELECT cursor FROM central_member_removals")
        .fetch_one(&store.pool)
        .await?;
    sqlx::query("CREATE TRIGGER fail_snapshot_checkpoint BEFORE UPDATE ON central_member_removals BEGIN SELECT RAISE(ABORT,'injected checkpoint failure'); END").execute(&store.pool).await?;
    assert!(store.advance_member_removal_snapshots(&key).await.is_err());
    assert_eq!(
        sqlx::query_scalar::<_, String>(
            "SELECT json_extract(event_json,'$.display_name') FROM room_events WHERE seq=?"
        )
        .bind(event.seq)
        .fetch_one(&store.pool)
        .await?,
        "Private Name"
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT cursor FROM central_member_removals")
            .fetch_one(&store.pool)
            .await?,
        cursor
    );
    sqlx::query("DROP TRIGGER fail_snapshot_checkpoint")
        .execute(&store.pool)
        .await?;
    let before: i64 = sqlx::query_scalar("SELECT total_changes()")
        .fetch_one(&store.pool)
        .await?;
    store.advance_member_removal_snapshots(&key).await?;
    let rows = sqlx::query_scalar::<_, i64>("SELECT total_changes()")
        .fetch_one(&store.pool)
        .await?
        - before;
    assert_eq!(rows, 2);
    println!("snapshot metadata_rows={rows}; stored replay exceeds 128KiB");
    Ok(())
}
