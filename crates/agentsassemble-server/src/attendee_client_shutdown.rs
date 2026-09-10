//! Positive local shutdown precedes exact remote cleanup receipts.
use crate::{AttendeeClientError, AttendeeRuntime, RoomAttendeeClient};
use agentsassemble_persistence::{AttendeeCleanupDelivery, AttendeeCleanupReport};
use std::{future::Future, time::Duration};
use uuid::Uuid;

/// Stops the caller-owned provider and resolves departure/cleanup using retained request IDs.
/// A failure leaves local artifacts with the caller; it never proves remote departure.
///
/// # Errors
/// Preserves unconfirmed provider cleanup, rejected custody, and bounded receipt uncertainty.
pub async fn shutdown_attendee(
    client: &RoomAttendeeClient,
    mut runtime: Option<&mut AttendeeRuntime>,
    remote_stop: Option<AttendeeCleanupDelivery>,
) -> Result<(), AttendeeClientError> {
    if let Some(runtime) = runtime.as_deref_mut() {
        runtime.stop().await?;
    }
    tokio::time::timeout(Duration::from_secs(30), async {
        let leave_id = Uuid::new_v4();
        let leave = if remote_stop.is_none() {
            retry(|| client.leave(leave_id)).await
        } else {
            Ok(())
        };
        let stop = match remote_stop {
            Some(stop) => Some(stop),
            None => retry(|| client.cleanup()).await?,
        };
        if let Some(stop) = stop {
            if let Some(runtime) = runtime.as_deref_mut() {
                runtime.verify_cleanup(&stop)?;
            } else if !stop.runtime_handle_id.is_empty()
                || !stop.runtime_owner_id.is_empty()
                || !stop.runtime_lease_token.is_empty()
            {
                return Err(AttendeeClientError::local(
                    "attendee_cleanup_authority_mismatch",
                ));
            }
            let report = AttendeeCleanupReport {
                request_id: Uuid::new_v4(),
                stopped: stop.clone(),
            };
            retry(|| client.report_cleanup(&report)).await?;
            if let Some(runtime) = runtime {
                runtime.acknowledge_cleanup(Some(&stop)).await?;
            }
            Ok(())
        } else {
            leave?;
            if let Some(runtime) = runtime {
                runtime.acknowledge_cleanup(None).await?;
            }
            Ok(())
        }
    })
    .await
    .map_err(|_| AttendeeClientError::local("attendee_cleanup_unresolved"))?
}

async fn retry<T, F: Future<Output = Result<T, AttendeeClientError>>>(
    mut operation: impl FnMut() -> F,
) -> Result<T, AttendeeClientError> {
    loop {
        match operation().await {
            Err(error) if error.is_retryable() => {
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
            outcome => return outcome,
        }
    }
}
