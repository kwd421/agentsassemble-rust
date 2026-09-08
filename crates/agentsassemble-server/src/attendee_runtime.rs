use agentsassemble_persistence::{AttendeeAdmission, AttendeeAdmissionRequest, PersistenceError};
use tokio::sync::{mpsc, oneshot};

use super::{RoomCommandOwners, RoomMutation, RoomRuntime};
use crate::event_publication::{PublicationAttempt, publish_durable_room_events};

pub(super) struct AttendeeAdmissionCommand {
    invite_fingerprint: [u8; 32],
    client_fingerprint: [u8; 32],
    request_id: uuid::Uuid,
    provider_kind: String,
    display_name: String,
    reply: oneshot::Sender<Result<AttendeeAdmission, PersistenceError>>,
}

impl RoomRuntime {
    /// Admits external provider custody through the existing bounded room queue.
    ///
    /// # Errors
    /// Rejects unsupported providers, unavailable invitations, queue pressure and lost responses.
    pub async fn admit_attendee(
        &self,
        request: AttendeeAdmissionRequest<'_>,
    ) -> Result<AttendeeAdmission, PersistenceError> {
        let provider_kind = agentsassemble_provider::registered_provider_kind(
            request.provider_kind,
        )
        .ok_or_else(|| {
            rejected(
                "unsupported_provider",
                "The attendee provider is unsupported.",
            )
        })?;
        let room_id = self
            .store
            .attendee_admission_room_id(request.invite_fingerprint)
            .await?
            .ok_or_else(|| {
                rejected(
                    "invite_unavailable",
                    "The attendee invitation is unavailable.",
                )
            })?;
        let handle = self.handle(&room_id).await;
        let (reply, response) = oneshot::channel();
        handle
            .mutations
            .try_send(RoomMutation::AttendeeAdmission(AttendeeAdmissionCommand {
                invite_fingerprint: *request.invite_fingerprint,
                client_fingerprint: *request.client_fingerprint,
                request_id: request.request_id,
                provider_kind: provider_kind.to_owned(),
                display_name: request.display_name.to_owned(),
                reply,
            }))
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => {
                    rejected("room_busy", "Room mutation queue is full.")
                }
                mpsc::error::TrySendError::Closed(_) => {
                    rejected("room_unavailable", "Room mutation task stopped.")
                }
            })?;
        response
            .await
            .map_err(|_| PersistenceError::CommandUnresolved {
                code: "admission_response_lost".into(),
                message: "Room admission response was lost; retry the same admission identity."
                    .to_owned(),
            })?
    }
}

pub(super) async fn admit(
    owners: &RoomCommandOwners<'_>,
    room_id: &str,
    command: AttendeeAdmissionCommand,
) -> Option<PublicationAttempt> {
    let admission = owners
        .store
        .admit_attendee(
            AttendeeAdmissionRequest {
                invite_fingerprint: &command.invite_fingerprint,
                client_fingerprint: &command.client_fingerprint,
                request_id: command.request_id,
                provider_kind: &command.provider_kind,
                display_name: &command.display_name,
            },
            chrono::Utc::now(),
        )
        .await;
    let publication = if admission.is_ok() {
        Some(publish_durable_room_events(owners.store, owners.event_tx, room_id).await)
    } else {
        None
    };
    let _ = command.reply.send(admission);
    publication
}

fn rejected(code: &'static str, message: &str) -> PersistenceError {
    PersistenceError::CommandRejected {
        code: code.into(),
        message: message.to_owned(),
    }
}
