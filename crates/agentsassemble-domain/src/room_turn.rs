use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::{clean_single_line, has_visible_text};

static DICE_NOTATION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^\s*(\d{0,3})d(\d{1,4})([+-]\d{1,5})?\s*$")
        .unwrap_or_else(|error| panic!("valid dice notation regex: {error}"))
});

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoomInputDeliveryKind {
    OrderedObservation,
    AmbientObservation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueuedRoomInput {
    pub event_id: String,
    pub delivery_kind: RoomInputDeliveryKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoomRandomRequest {
    Roll {
        notation: String,
        count: u32,
        sides: u32,
        modifier: i32,
        reason: String,
    },
    Choose {
        options: Vec<String>,
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub enum RoomRandomResult {
    RollDice {
        notation: String,
        rolls: Vec<u32>,
        modifier: i32,
        total: i64,
    },
    ChooseRandom {
        choice: String,
        index: usize,
        option_count: usize,
        options: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{message}")]
pub struct RoomRandomError {
    pub message: String,
}

impl RoomRandomRequest {
    /// Parses the exact shared human/provider room-random contract.
    ///
    /// # Errors
    ///
    /// Rejects unknown fields, wrong types, invalid notation, and unbounded options.
    pub fn parse(action: &str, payload: &Value) -> Result<Self, RoomRandomError> {
        let object = payload
            .as_object()
            .ok_or_else(|| rejected("Room randomness payload must be an object."))?;
        let reason = match object.get("reason") {
            None => String::new(),
            Some(Value::String(value)) => clean_single_line(value, 200),
            Some(_) => return Err(rejected("Room randomness reason must be text.")),
        };
        match action {
            "room.random.roll" => {
                require_keys(object.keys(), &["notation", "reason"])?;
                let notation = object
                    .get("notation")
                    .and_then(Value::as_str)
                    .ok_or_else(|| rejected("Dice notation must be text."))?;
                parse_dice(notation, reason)
            }
            "room.random.choose" => {
                require_keys(object.keys(), &["options", "reason"])?;
                let values = object
                    .get("options")
                    .and_then(Value::as_array)
                    .ok_or_else(|| rejected("Random choice requires a list of options."))?;
                if !(2..=50).contains(&values.len()) {
                    return Err(rejected(
                        "Random choice requires 2 to 50 non-empty options.",
                    ));
                }
                let options = values
                    .iter()
                    .map(|value| {
                        value
                            .as_str()
                            .map(|value| clean_single_line(value, 200))
                            .filter(|value| !value.is_empty() && has_visible_text(value))
                            .ok_or_else(|| {
                                rejected("Random choice requires 2 to 50 non-empty options.")
                            })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Self::Choose { options, reason })
            }
            _ => Err(rejected("Unsupported room randomness operation.")),
        }
    }

    #[must_use]
    pub const fn operation(&self) -> &'static str {
        match self {
            Self::Roll { .. } => "roll_dice",
            Self::Choose { .. } => "choose_random",
        }
    }

    #[must_use]
    pub fn reason(&self) -> &str {
        match self {
            Self::Roll { reason, .. } | Self::Choose { reason, .. } => reason,
        }
    }
}

fn parse_dice(value: &str, reason: String) -> Result<RoomRandomRequest, RoomRandomError> {
    let captures = DICE_NOTATION
        .captures(value)
        .ok_or_else(|| rejected("Dice notation must look like d20, 2d6, or 1d20+3."))?;
    let count = captures
        .get(1)
        .map_or("1", |value| value.as_str())
        .parse::<u32>()
        .map_err(|_| rejected("A dice roll must use between 1 and 100 dice."))?;
    let sides = captures[2]
        .parse::<u32>()
        .map_err(|_| rejected("Dice must have between 2 and 1000 sides."))?;
    let modifier = captures
        .get(3)
        .map_or("0", |value| value.as_str())
        .parse::<i32>()
        .map_err(|_| rejected("The dice modifier is out of range."))?;
    if !(1..=100).contains(&count) {
        return Err(rejected("A dice roll must use between 1 and 100 dice."));
    }
    if !(2..=1000).contains(&sides) {
        return Err(rejected("Dice must have between 2 and 1000 sides."));
    }
    if !(-100_000..=100_000).contains(&modifier) {
        return Err(rejected("The dice modifier is out of range."));
    }
    let notation = if modifier == 0 {
        format!("{count}d{sides}")
    } else {
        format!("{count}d{sides}{modifier:+}")
    };
    Ok(RoomRandomRequest::Roll {
        notation,
        count,
        sides,
        modifier,
        reason,
    })
}

fn require_keys<'a>(
    keys: impl Iterator<Item = &'a String>,
    allowed: &[&str],
) -> Result<(), RoomRandomError> {
    if let Some(key) = keys
        .into_iter()
        .find(|key| !allowed.contains(&key.as_str()))
    {
        return Err(rejected(format!("Unknown room randomness field `{key}`.")));
    }
    Ok(())
}

fn rejected(message: impl Into<String>) -> RoomRandomError {
    RoomRandomError {
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{RoomRandomRequest, RoomRandomResult};

    #[test]
    fn room_random_parser_is_shared_strict_and_bounded() {
        assert_eq!(
            RoomRandomRequest::parse(
                "room.random.roll",
                &json!({"notation": " 2D6+3 ", "reason": " initiative "}),
            )
            .unwrap_or_else(|error| panic!("parse dice request: {error}")),
            RoomRandomRequest::Roll {
                notation: "2d6+3".to_owned(),
                count: 2,
                sides: 6,
                modifier: 3,
                reason: "initiative".to_owned(),
            }
        );
        assert!(
            RoomRandomRequest::parse(
                "room.random.roll",
                &json!({"notation": "101d6", "unknown": true}),
            )
            .is_err()
        );
        assert!(
            RoomRandomRequest::parse("room.random.choose", &json!({"options": ["only one"]}),)
                .is_err()
        );
        assert!(
            RoomRandomRequest::parse(
                "room.random.choose",
                &json!({"options": ["first", "\u{200b}"]}),
            )
            .is_err()
        );
    }

    #[test]
    fn room_random_results_have_one_typed_operation() {
        let encoded = serde_json::to_value(RoomRandomResult::ChooseRandom {
            choice: "north".to_owned(),
            index: 0,
            option_count: 2,
            options: vec!["north".to_owned(), "south".to_owned()],
        })
        .unwrap_or_else(|error| panic!("encode random result: {error}"));
        assert_eq!(encoded["operation"], "choose_random");
        assert!(encoded.get("roll_dice").is_none());
    }
}
