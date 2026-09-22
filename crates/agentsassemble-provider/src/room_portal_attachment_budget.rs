use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use agentsassemble_domain::{MAX_ATTACHMENT_BYTES, MAX_MESSAGE_ATTACHMENTS_PER_EVENT};
use uuid::Uuid;

use super::PortalState;
use crate::room_attachment::{ProviderAttachmentReadAuthority, ProviderAttachmentReadIngress};

const ATTEMPTS_PER_ATTACHMENT: usize = 2;

#[derive(Debug, Default)]
pub(crate) struct AttachmentReadBudget {
    attempts_by_id: BTreeMap<String, usize>,
    pending: BTreeMap<Uuid, String>,
    successful_reads: BTreeMap<String, usize>,
    successful_bytes: usize,
}

impl AttachmentReadBudget {
    pub(super) fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }
}

pub(crate) struct AttachmentReadReservation {
    state: Arc<Mutex<PortalState>>,
    turn_generation: Uuid,
    reservation_id: Uuid,
    attachment_id: String,
    resolved: bool,
}

impl AttachmentReadReservation {
    pub(crate) fn complete(&mut self, size: usize) -> Result<(), String> {
        self.resolve(Some(size))
    }

    fn resolve(&mut self, successful_size: Option<usize>) -> Result<(), String> {
        if self.resolved {
            return Err("The room attachment reservation already ended.".to_owned());
        }
        self.resolved = true;
        let mut state = self
            .state
            .lock()
            .map_err(|_| "The shared room authority is unavailable.".to_owned())?;
        let Some(active) = state
            .active
            .as_mut()
            .filter(|active| active.turn_generation == self.turn_generation)
        else {
            return Err("The room attachment turn already ended.".to_owned());
        };
        if active.attachment_reads.pending.remove(&self.reservation_id)
            != Some(self.attachment_id.clone())
        {
            return Err("The room attachment reservation already ended.".to_owned());
        }
        let result = match successful_size {
            Some(size) if !active.closing => {
                let byte_limit = active
                    .attachment_ids
                    .len()
                    .min(MAX_MESSAGE_ATTACHMENTS_PER_EVENT)
                    .checked_mul(MAX_ATTACHMENT_BYTES)
                    .and_then(|limit| limit.checked_mul(ATTEMPTS_PER_ATTACHMENT))
                    .ok_or_else(|| "The room attachment byte limit is invalid.".to_owned())?;
                let next_bytes = active
                    .attachment_reads
                    .successful_bytes
                    .checked_add(size)
                    .ok_or_else(|| "The room attachment byte limit was exceeded.".to_owned())?;
                if !(1..=MAX_ATTACHMENT_BYTES).contains(&size) || next_bytes > byte_limit {
                    Err("The room attachment read result is invalid.".to_owned())
                } else {
                    *active
                        .attachment_reads
                        .successful_reads
                        .entry(self.attachment_id.clone())
                        .or_default() += 1;
                    active.attachment_reads.successful_bytes = next_bytes;
                    Ok(())
                }
            }
            Some(_) => Err("The room attachment turn already ended.".to_owned()),
            None => Ok(()),
        };
        let remove_tombstone = active.closing && !active.has_pending_operations();
        if remove_tombstone {
            state.active = None;
        }
        result
    }
}

impl Drop for AttachmentReadReservation {
    fn drop(&mut self) {
        if !self.resolved {
            let _ = self.resolve(None);
        }
    }
}

pub(crate) fn reserve_attachment_read(
    state: &Arc<Mutex<PortalState>>,
    attachment_id: &str,
) -> Result<
    (
        ProviderAttachmentReadAuthority,
        ProviderAttachmentReadIngress,
        AttachmentReadReservation,
    ),
    String,
> {
    let mut portal = state
        .lock()
        .map_err(|_| "The shared room authority is unavailable.".to_owned())?;
    let active = portal
        .active
        .as_mut()
        .ok_or_else(|| "No active room observation.".to_owned())?;
    if active.closing || active.outcome.is_some() || !active.attachment_ids.contains(attachment_id)
    {
        return Err("The attachment is not available to this room turn.".to_owned());
    }
    let ingress = active
        .attachment_ingress
        .clone()
        .ok_or_else(|| "The room attachment owner is unavailable.".to_owned())?;
    if active
        .attachment_reads
        .pending
        .values()
        .any(|pending_id| pending_id == attachment_id)
    {
        return Err("The attachment is already being read.".to_owned());
    }
    let attachment_limit = active
        .attachment_ids
        .len()
        .min(MAX_MESSAGE_ATTACHMENTS_PER_EVENT);
    if attachment_limit == 0 {
        return Err("This turn has no room attachments.".to_owned());
    }
    let attempts = active
        .attachment_reads
        .attempts_by_id
        .entry(attachment_id.to_owned())
        .or_default();
    if *attempts >= ATTEMPTS_PER_ATTACHMENT {
        return Err("This turn reached its room-attachment read limit.".to_owned());
    }
    *attempts += 1;
    let reservation_id = Uuid::new_v4();
    active
        .attachment_reads
        .pending
        .insert(reservation_id, attachment_id.to_owned());
    let authority = ProviderAttachmentReadAuthority {
        session_id: active.authority.session_id.clone(),
        turn_id: active.authority.turn_id.clone(),
        input_up_to_seq: active.authority.input_up_to_seq,
        turn_generation: active.authority.durable_turn_generation,
        execution_id: active.authority.execution_id.clone(),
    };
    Ok((
        authority,
        ingress,
        AttachmentReadReservation {
            state: state.clone(),
            turn_generation: active.turn_generation,
            reservation_id,
            attachment_id: attachment_id.to_owned(),
            resolved: false,
        },
    ))
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;
    use crate::room_portal::{ActiveObservation, TurnAuthority};

    const ATTACHMENT_ID: &str = "ma_11111111111111111111111111111111";

    fn fixture_state() -> Arc<Mutex<PortalState>> {
        let (ingress, _commands) = ProviderAttachmentReadIngress::channel(1);
        Arc::new(Mutex::new(PortalState {
            active: Some(ActiveObservation {
                authority: TurnAuthority {
                    session_id: "agent-1".to_owned(),
                    turn_id: "turn-1".to_owned(),
                    input_up_to_seq: 7,
                    durable_turn_generation: 3,
                    execution_id: "00000000-0000-4000-8000-000000000003".to_owned(),
                    allowed_agent_ids: Vec::new(),
                },
                room_view: format!("Attachment `{ATTACHMENT_ID}`"),
                attachment_ids: HashSet::from([ATTACHMENT_ID.to_owned()]),
                attachment_ingress: Some(ingress),
                attachment_reads: AttachmentReadBudget::default(),
                turn_generation: Uuid::new_v4(),
                receipt_generation: None,
                outcome: None,
                tabletop_tools: false,
                tool_ingress: None,
                tool_reservations: BTreeMap::new(),
                successful_tool_results: 0,
                closing: false,
            }),
        }))
    }

    #[test]
    fn duplicate_concurrent_success_and_failed_attempts_are_bounded() {
        let state = fixture_state();
        let (_, _, mut first) = reserve_attachment_read(&state, ATTACHMENT_ID)
            .unwrap_or_else(|error| panic!("reserve first attachment read: {error}"));
        assert!(reserve_attachment_read(&state, ATTACHMENT_ID).is_err());
        first
            .complete(4)
            .unwrap_or_else(|error| panic!("complete first attachment read: {error}"));
        let (_, _, mut retry) = reserve_attachment_read(&state, ATTACHMENT_ID)
            .unwrap_or_else(|error| panic!("reserve attachment retry: {error}"));
        retry
            .complete(4)
            .unwrap_or_else(|error| panic!("complete attachment retry: {error}"));
        assert!(reserve_attachment_read(&state, ATTACHMENT_ID).is_err());
        let locked = state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let active = locked
            .active
            .as_ref()
            .unwrap_or_else(|| panic!("active observation"));
        assert_eq!(active.attachment_reads.attempts_by_id[ATTACHMENT_ID], 2);
        assert_eq!(active.attachment_reads.successful_reads[ATTACHMENT_ID], 2);
        assert_eq!(active.attachment_reads.successful_bytes, 8);
        drop(locked);

        let failed = fixture_state();
        for _ in 0..ATTEMPTS_PER_ATTACHMENT {
            let (_, _, reservation) = reserve_attachment_read(&failed, ATTACHMENT_ID)
                .unwrap_or_else(|error| panic!("reserve failed attachment read: {error}"));
            drop(reservation);
        }
        assert!(reserve_attachment_read(&failed, ATTACHMENT_ID).is_err());
    }

    #[test]
    fn abort_tombstone_lives_until_cancelled_read_releases() {
        let state = fixture_state();
        let (_, _, reservation) = reserve_attachment_read(&state, ATTACHMENT_ID)
            .unwrap_or_else(|error| panic!("reserve attachment read: {error}"));
        {
            let mut locked = state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            locked
                .active
                .as_mut()
                .unwrap_or_else(|| panic!("active observation"))
                .closing = true;
        }
        assert!(
            state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .active
                .is_some()
        );
        drop(reservation);
        assert!(
            state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .active
                .is_none()
        );
    }
}
