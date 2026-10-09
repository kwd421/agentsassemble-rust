//! Committed person work is resumed by the existing runtime lifecycle owner.
use agentsassemble_persistence::{MemberRemovalKey, PersistenceError, SqliteStore};
use agentsassemble_provider::ProviderAdapter;
use tokio_util::sync::CancellationToken;

use crate::{RoomRuntime, owner_session_lifetime::OwnerSessionLifetimes};

pub(crate) async fn reconcile(
    store: &SqliteStore,
    adapter: &ProviderAdapter,
    rooms: &RoomRuntime,
    owners: &OwnerSessionLifetimes,
    cancellation: &CancellationToken,
) -> Result<(), PersistenceError> {
    let mut cursor = None;
    loop {
        let page = store.pending_member_removals(cursor.as_ref()).await?;
        if page.is_empty() {
            return Ok(());
        }
        for key in page {
            drive(store, adapter, rooms, owners, &key, cancellation).await?;
            if cancellation.is_cancelled() {
                return Ok(());
            }
            cursor = Some(key);
        }
    }
}

async fn drive(
    store: &SqliteStore,
    adapter: &ProviderAdapter,
    rooms: &RoomRuntime,
    owners: &OwnerSessionLifetimes,
    key: &MemberRemovalKey,
    cancellation: &CancellationToken,
) -> Result<(), PersistenceError> {
    while !cancellation.is_cancelled() {
        let phase = store.member_removal_phase(key).await?;
        match phase.as_str() {
            "owner_sessions" | "owner_pairings" | "human_sessions" => {
                let page = store.advance_member_removal_authority(key).await?;
                owners.revoke(&page.owner_fingerprints);
                rooms.publish_session_revocations(&page.room_sessions).await;
            }
            "companions" => {
                let page = store.advance_member_removal_companions(key).await?;
                rooms.publish_session_revocations(&page.room_sessions).await;
                rooms.notify_committed_events(&page.events).await;
            }
            "member_invites"
            | "profile"
            | "avatars"
            | "room_events"
            | "member_admissions"
            | "human_room_sessions"
            | "command_results"
            | "room_create_results"
            | "room_delete_results"
            | "lifecycle_command_reservations"
            | "search" => {
                store.advance_member_removal_snapshots(key).await?;
            }
            "rooms" | "room_publish" => {
                if let Some(page) = store.advance_member_removal_room(key).await? {
                    // The existing room publisher replies only after its durable drain.
                    rooms
                        .publish_then_resume_assigned_turns(&page.room_id, Vec::new())
                        .await?;
                    store.finish_member_removal_room(key, &page).await?;
                }
            }
            "effects" | "effect_owners" | "effect_pairings" | "effect_humans"
            | "effect_companions" => {
                let page = store.member_removal_effect(key).await?;
                owners.revoke(&page.owner_fingerprints);
                rooms.publish_session_revocations(&page.room_sessions).await;
                if let Some(cleanup) = &page.cleanup {
                    let commit = Box::pin(crate::room_runtime_cleanup::attempt_cleanup(
                        store, adapter, cleanup,
                    ))
                    .await?;
                    if let Some(commit) = commit {
                        rooms
                            .publish_then_resume_assigned_turns(
                                &cleanup.room_id,
                                commit.next_assignments,
                            )
                            .await?;
                    }
                }
                store.finish_member_removal_effect(key, &page).await?;
            }
            "complete" => return Ok(()),
            _ => {
                return Err(PersistenceError::CommandUnresolved {
                    code: "account_removal_phase_unhandled".into(),
                    message: "Person removal has an unsupported committed phase.".into(),
                });
            }
        }
        tokio::task::yield_now().await;
    }
    Ok(())
}

/// Existing wake owner records failures without claiming host completion.
pub(crate) async fn reconcile_wake(
    store: &SqliteStore,
    adapter: &ProviderAdapter,
    rooms: &RoomRuntime,
    owners: &OwnerSessionLifetimes,
    cancellation: &CancellationToken,
) {
    if let Err(error) = reconcile(store, adapter, rooms, owners, cancellation).await {
        tracing::warn!(
            code = crate::room_command_execution::persistence_error_code(&error),
            "person removal remains durable and unconfirmed"
        );
    }
}
