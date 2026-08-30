use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{CommandRejection, RoomEvent};

pub const ROOM_HISTORY_MAX_EVENTS: i64 = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoomHistoryRequest {
    pub before_seq: i64,
    pub limit: i64,
}

impl RoomHistoryRequest {
    /// Parses the canonical browser history payload while preserving the original bounded defaults.
    ///
    /// # Errors
    ///
    /// Rejects non-object payloads, unknown fields, and non-integer cursor or limit values.
    pub fn from_payload(payload: &Value) -> Result<Self, CommandRejection> {
        let object = payload
            .as_object()
            .ok_or_else(|| CommandRejection::new("bad_request", "payload must be an object."))?;
        if object
            .keys()
            .any(|key| !matches!(key.as_str(), "before_seq" | "limit"))
        {
            return Err(CommandRejection::new(
                "bad_request",
                "room.history accepts only before_seq and limit fields.",
            ));
        }
        let before_seq = integer_field(object.get("before_seq"), "before_seq")?
            .unwrap_or_default()
            .max(0);
        let limit = integer_field(object.get("limit"), "limit")?
            .unwrap_or(ROOM_HISTORY_MAX_EVENTS)
            .clamp(1, ROOM_HISTORY_MAX_EVENTS);
        Ok(Self { before_seq, limit })
    }
}

fn integer_field(
    value: Option<&Value>,
    field: &'static str,
) -> Result<Option<i64>, CommandRejection> {
    value
        .map(|value| {
            value.as_i64().ok_or_else(|| {
                CommandRejection::new(
                    "bad_request",
                    format!("room.history {field} must be an integer."),
                )
            })
        })
        .transpose()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoomHistoryPage {
    pub events: Vec<RoomEvent>,
    pub oldest_seq: i64,
    pub last_seq: i64,
    pub has_more_before: bool,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{ROOM_HISTORY_MAX_EVENTS, RoomHistoryRequest};

    #[test]
    fn request_preserves_bounded_original_defaults_without_aliases() {
        assert_eq!(
            RoomHistoryRequest::from_payload(&json!({})),
            Ok(RoomHistoryRequest {
                before_seq: 0,
                limit: ROOM_HISTORY_MAX_EVENTS,
            })
        );
        assert_eq!(
            RoomHistoryRequest::from_payload(&json!({"before_seq": -4, "limit": 500})),
            Ok(RoomHistoryRequest {
                before_seq: 0,
                limit: ROOM_HISTORY_MAX_EVENTS,
            })
        );
        assert!(RoomHistoryRequest::from_payload(&json!({"beforeSeq": 2})).is_err());
        assert!(RoomHistoryRequest::from_payload(&json!({"before_seq": 2.5})).is_err());
        assert!(RoomHistoryRequest::from_payload(&json!({"limit": "2"})).is_err());
    }
}
