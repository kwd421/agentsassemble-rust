use agentsassemble_domain::{AuthenticatedPrincipal, RoomHistoryPage, RoomHistoryRequest};
use agentsassemble_persistence::{PersistenceError, SqliteStore};
use agentsassemble_protocol::{CommandAck, CommandResolution, RoomAction, ServerFrame};
use serde_json::Value;

use crate::{
    authenticated_channel::encode_server_frame,
    room_command_result::{CommandFailure, validate_command_envelope},
    socket_admission::SocketAdmission,
};

pub(crate) async fn read_history_frame(
    store: &SqliteStore,
    admission: &SocketAdmission,
    principal: &AuthenticatedPrincipal,
    request_id: &str,
    payload: &Value,
) -> Result<ServerFrame, CommandFailure> {
    validate_command_envelope(request_id).map_err(CommandFailure::rejected)?;
    let request =
        RoomHistoryRequest::from_payload(payload).map_err(CommandFailure::domain_rejected)?;
    let requested_events = usize::try_from(request.limit).map_err(|_| {
        CommandFailure::rejected(PersistenceError::CommandRejected {
            code: "bad_request",
            message: "room.history limit is outside the supported range.".to_owned(),
        })
    })?;
    if !admission.admit_history(principal, requested_events) {
        return Err(CommandFailure::rejected(
            PersistenceError::CommandRejected {
                code: "history_read_limited",
                message: "Room history read budget exceeded.".to_owned(),
            },
        ));
    }
    let page = store
        .room_history_page(principal, request)
        .await
        .map_err(CommandFailure::transactional)?;
    fit_history_ack(request_id, &page)
}

fn fit_history_ack(
    request_id: &str,
    page: &RoomHistoryPage,
) -> Result<ServerFrame, CommandFailure> {
    let full = history_ack(request_id, page, 0)?;
    if encode_server_frame(&full).is_ok() {
        return Ok(full);
    }
    if page.events.len() < 2 {
        return Err(oversize_failure());
    }

    // Exact frame size depends on JSON escaping and request-id length. Search the bounded
    // 200-event page rather than estimating bytes or introducing a second transport limit.
    let mut first = 1;
    let mut last = page.events.len() - 1;
    let mut fitted = None;
    while first <= last {
        let dropped = first + (last - first) / 2;
        let candidate = history_ack(request_id, page, dropped)?;
        if encode_server_frame(&candidate).is_ok() {
            fitted = Some(candidate);
            if dropped == 1 {
                break;
            }
            last = dropped - 1;
        } else {
            first = dropped + 1;
        }
    }
    fitted.ok_or_else(oversize_failure)
}

fn history_ack(
    request_id: &str,
    page: &RoomHistoryPage,
    dropped: usize,
) -> Result<ServerFrame, CommandFailure> {
    let events = page.events[dropped..].to_vec();
    let result = RoomHistoryPage {
        oldest_seq: events.first().map_or(0, |event| event.seq),
        events,
        last_seq: page.last_seq,
        has_more_before: dropped > 0 || page.has_more_before,
    };
    let result = serde_json::to_value(result)
        .map_err(PersistenceError::from)
        .map_err(CommandFailure::unresolved)?;
    Ok(ServerFrame::Ack(CommandAck {
        request_id: request_id.to_owned(),
        accepted: true,
        resolution: CommandResolution::Committed,
        action: RoomAction::RoomHistory.as_str().to_owned(),
        result,
        deduplicated: false,
    }))
}

fn oversize_failure() -> CommandFailure {
    CommandFailure::rejected(PersistenceError::CommandRejected {
        code: "response_too_large",
        message: "A canonical room history event exceeds the WebSocket frame limit.".to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use agentsassemble_domain::{Actor, RoomEvent, RoomHistoryPage};

    use super::fit_history_ack;
    use crate::authenticated_channel::encode_server_frame;

    fn event(seq: i64, content: &str) -> RoomEvent {
        RoomEvent {
            v: 1,
            id: format!("event-{seq}"),
            room_id: "general".to_owned(),
            seq,
            event_type: "message_final".to_owned(),
            actor: Actor {
                participant_id: "room-system".to_owned(),
                participant_type: "system".to_owned(),
            },
            participant_id: None,
            participant_type: None,
            actor_id: None,
            actor_type: None,
            display_name: None,
            content: Some(content.to_owned()),
            message_kind: None,
            created_at: chrono::Utc::now(),
            extra: BTreeMap::new(),
        }
    }

    fn result(frame: agentsassemble_protocol::ServerFrame) -> RoomHistoryPage {
        let agentsassemble_protocol::ServerFrame::Ack(ack) = frame else {
            panic!("history result was not an ACK");
        };
        serde_json::from_value(ack.result)
            .unwrap_or_else(|error| panic!("decode history result: {error}"))
    }

    #[test]
    fn small_page_keeps_all_events() {
        let page = RoomHistoryPage {
            events: (1..=200).map(|seq| event(seq, "small")).collect(),
            oldest_seq: 1,
            last_seq: 200,
            has_more_before: false,
        };
        let frame = fit_history_ack("history-small", &page)
            .unwrap_or_else(|failure| panic!("fit small history: {}", failure.error));
        assert!(encode_server_frame(&frame).is_ok());
        let result = result(frame);
        assert_eq!(result.events.len(), 200);
        assert_eq!(result.oldest_seq, 1);
        assert!(!result.has_more_before);
    }

    #[test]
    fn large_page_drops_only_earliest_events_and_fits_exact_encoder() {
        let page = RoomHistoryPage {
            events: (1..=200)
                .map(|seq| event(seq, &"x".repeat(12_000)))
                .collect(),
            oldest_seq: 1,
            last_seq: 200,
            has_more_before: false,
        };
        let frame = fit_history_ack(&"r".repeat(128), &page)
            .unwrap_or_else(|failure| panic!("fit large history: {}", failure.error));
        assert!(encode_server_frame(&frame).is_ok());
        let result = result(frame);
        assert!(result.events.len() < 200);
        assert_eq!(result.events.last().map(|event| event.seq), Some(200));
        assert_eq!(
            result.oldest_seq,
            result.events.first().map_or(0, |event| event.seq)
        );
        assert!(result.has_more_before);
        assert!(
            result
                .events
                .windows(2)
                .all(|pair| pair[0].seq + 1 == pair[1].seq)
        );
    }
}
