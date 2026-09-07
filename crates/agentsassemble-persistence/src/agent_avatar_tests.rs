use crate::RoomMutationAuthority::TrustedPrincipal;
use crate::{
    SqliteStore,
    agent_lifecycle::tests::{AGENT_ID, fixture},
};
use agentsassemble_domain::{
    LOCAL_OPERATOR_PARTICIPANT_ID, LOCAL_OPERATOR_USER_ID, UserProfilePatch,
};
use image::{DynamicImage, ImageBuffer, ImageFormat, Rgba};
use serde_json::json;
use std::io::Cursor;

fn png() -> Result<Vec<u8>, image::ImageError> {
    let mut encoded = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(ImageBuffer::from_pixel(2, 2, Rgba([1, 2, 3, 255])))
        .write_to(&mut encoded, ImageFormat::Png)?;
    Ok(encoded.into_inner())
}

#[tokio::test]
async fn agent_avatar_custody_replaces_exact_references_and_survives_restart()
-> Result<(), Box<dyn std::error::Error>> {
    let (store, principal, directory) = fixture().await;
    let authority = store
        .authorize_local_room_manager(
            "general",
            LOCAL_OPERATOR_USER_ID,
            LOCAL_OPERATOR_PARTICIPANT_ID,
        )
        .await?;
    let human = bound_human_avatar(&store).await?;
    let appearance = store
        .store_pending_room_appearance_asset(&authority, "room.png", "image/png", png()?)
        .await?;
    let first = store
        .store_agent_avatar(&authority, AGENT_ID, "first.png", "image/png", png()?)
        .await?;
    assert!(store.agent_avatar(&first.id).await.is_err());
    let bind = json!({"agent_id": AGENT_ID, "avatar_image_url": first.url});
    let outcome = store
        .execute_agent_profile_update(TrustedPrincipal(&principal), "avatar-first", &bind)
        .await?;
    assert_eq!(outcome.result["participant"]["avatar_image_url"], first.url);
    assert_eq!(store.agent_avatar(&first.id).await?.metadata, first);
    let discarded = store
        .store_agent_avatar(&authority, AGENT_ID, "discarded.png", "image/png", png()?)
        .await?;
    let next = store
        .store_agent_avatar(&authority, AGENT_ID, "next.png", "image/png", png()?)
        .await?;
    assert!(
        store
            .execute_agent_profile_update(
                TrustedPrincipal(&principal),
                "discarded",
                &json!({"agent_id": AGENT_ID, "avatar_image_url": discarded.url})
            )
            .await
            .is_err()
    );
    assert!(
        store
            .execute_agent_profile_update(
                TrustedPrincipal(&principal),
                "human-ref",
                &json!({"agent_id": AGENT_ID, "avatar_image_url": human.url})
            )
            .await
            .is_err()
    );
    assert!(
        store
            .execute_agent_profile_update(
                TrustedPrincipal(&principal),
                "room-ref",
                &json!({"agent_id": AGENT_ID, "avatar_image_url": appearance.url})
            )
            .await
            .is_err()
    );
    assert_eq!(store.agent_avatar(&first.id).await?.metadata, first);
    let next_payload = json!({"agent_id": AGENT_ID, "avatar_image_url": next.url});
    let next_outcome = store
        .execute_agent_profile_update(TrustedPrincipal(&principal), "avatar-next", &next_payload)
        .await?;
    assert!(store.agent_avatar(&first.id).await.is_err());
    assert_eq!(store.profile_attachment(&human.id).await?.metadata, human);
    assert_eq!(
        store
            .pending_room_appearance_asset(&authority, &appearance.id)
            .await?
            .metadata,
        appearance
    );
    store.pool.close().await;
    drop(store);
    let reopened = SqliteStore::open_path(&directory.path().join("runtime.sqlite3")).await?;
    let replay = reopened
        .execute_agent_profile_update(TrustedPrincipal(&principal), "avatar-next", &next_payload)
        .await?;
    assert!(replay.deduplicated);
    assert_eq!(replay.result, next_outcome.result);
    assert_eq!(reopened.agent_avatar(&next.id).await?.metadata, next);
    reopened
        .execute_agent_profile_update(
            TrustedPrincipal(&principal),
            "avatar-clear",
            &json!({"agent_id": AGENT_ID, "avatar_image_url": ""}),
        )
        .await?;
    assert!(reopened.agent_avatar(&next.id).await.is_err());
    assert_eq!(
        reopened.profile_attachment(&human.id).await?.metadata,
        human
    );
    Ok(())
}

#[tokio::test]
async fn agent_avatar_rejects_stale_authority_expired_and_other_session_references()
-> Result<(), Box<dyn std::error::Error>> {
    let (store, principal, _directory) = fixture().await;
    let authority = store
        .authorize_local_room_manager(
            "general",
            LOCAL_OPERATOR_USER_ID,
            LOCAL_OPERATOR_PARTICIPANT_ID,
        )
        .await?;
    let mut stale = authority.clone();
    stale.room_uid = uuid::Uuid::new_v4();
    assert!(
        store
            .store_agent_avatar(&stale, AGENT_ID, "image.png", "image/png", png()?)
            .await
            .is_err()
    );
    assert!(
        store
            .store_agent_avatar(&authority, "absent", "image.png", "image/png", png()?)
            .await
            .is_err()
    );
    assert!(
        store
            .store_agent_avatar(&authority, AGENT_ID, "image.png", "image/png", vec![0])
            .await
            .is_err()
    );
    let expired = store
        .store_agent_avatar(&authority, AGENT_ID, "expired.png", "image/png", png()?)
        .await?;
    sqlx::query("UPDATE agent_avatar_assets SET expires_at = 1 WHERE asset_id = ?")
        .bind(&expired.id)
        .execute(&store.pool)
        .await?;
    assert!(
        store
            .execute_agent_profile_update(
                TrustedPrincipal(&principal),
                "expired",
                &json!({"agent_id": AGENT_ID, "avatar_image_url": expired.url})
            )
            .await
            .is_err()
    );
    // A second session has separate asset custody even within the same room and manager.
    let mut transaction = store.pool.begin().await?;
    let mut other = super::load_session(&mut transaction, "general", AGENT_ID).await?;
    let mut participant = super::load_participant(&mut transaction, "general", AGENT_ID).await?;
    other.public.session_id = "other-agent".into();
    other.public.participant_id = "other-agent".into();
    participant.participant_id = "other-agent".into();
    sqlx::query("INSERT INTO agent_sessions(room_id, session_id, session_json) VALUES ('general', 'other-agent', ?)")
        .bind(serde_json::to_string(&other)?).execute(&mut *transaction).await?;
    sqlx::query("INSERT INTO participants(room_id, participant_id, participant_json) VALUES ('general', 'other-agent', ?)")
        .bind(serde_json::to_string(&participant)?).execute(&mut *transaction).await?;
    transaction.commit().await?;
    let foreign = store
        .store_agent_avatar(
            &authority,
            "other-agent",
            "foreign.png",
            "image/png",
            png()?,
        )
        .await?;
    assert!(
        store
            .execute_agent_profile_update(
                TrustedPrincipal(&principal),
                "foreign",
                &json!({"agent_id": AGENT_ID, "avatar_image_url": foreign.url})
            )
            .await
            .is_err()
    );
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM agent_avatar_assets WHERE asset_id = ?")
            .bind(&foreign.id)
            .fetch_one(&store.pool)
            .await?;
    assert_eq!(count, 1);
    sqlx::query(
        "DELETE FROM agent_sessions WHERE room_id = 'general' AND session_id = 'other-agent'",
    )
    .execute(&store.pool)
    .await?;
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM agent_avatar_assets WHERE asset_id = ?")
            .bind(&foreign.id)
            .fetch_one(&store.pool)
            .await?;
    assert_eq!(count, 0);
    Ok(())
}

async fn bound_human_avatar(
    store: &SqliteStore,
) -> Result<crate::ProfileAttachmentMetadata, Box<dyn std::error::Error>> {
    let human = store
        .store_local_operator_profile_attachment("human.png", "image/png", png()?)
        .await?;
    let profile = store.local_operator_profile().await?;
    store
        .update_local_operator_profile(
            profile.revision,
            UserProfilePatch {
                avatar_image_url: Some(human.url.clone()),
                ..UserProfilePatch::default()
            },
        )
        .await?;
    Ok(human)
}
