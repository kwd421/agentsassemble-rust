use agentsassemble_domain::RoomEvent;
use agentsassemble_persistence::{AgentTurnAssignment, PersistenceError, SqliteStore};
use agentsassemble_provider::{ProviderAdapter, ProviderRoomToolIngress};
use tokio::{
    sync::{broadcast, oneshot},
    task::JoinSet,
};

use crate::{
    provider_recovery_tracker::ProviderRecoveryGuard,
    provider_turn::{ProviderTurnTaskResult, spawn_recovered_provider_turn},
};

pub(super) struct RecoveredAssignment {
    pub(super) assignment: AgentTurnAssignment,
    pub(super) guard: ProviderRecoveryGuard,
}

pub(super) struct RecoveredAssignments {
    pub(super) assignments: Vec<RecoveredAssignment>,
    pub(super) reply: oneshot::Sender<Result<(), PersistenceError>>,
}

pub(super) async fn publish_then_resume(
    store: &SqliteStore,
    event_tx: &broadcast::Sender<RoomEvent>,
    room_id: &str,
    turn_tasks: &mut JoinSet<ProviderTurnTaskResult>,
    provider_adapter: &ProviderAdapter,
    room_tool_ingress: &ProviderRoomToolIngress,
    recovery: RecoveredAssignments,
) {
    let result = crate::event_publication::drain_room_publications(store, event_tx, room_id).await;
    if result.is_ok() {
        for recovered in recovery.assignments {
            spawn_recovered_provider_turn(
                turn_tasks,
                store.clone(),
                provider_adapter.clone(),
                recovered.assignment,
                room_tool_ingress.clone(),
                recovered.guard,
            );
        }
    }
    let _ = recovery.reply.send(result);
}
