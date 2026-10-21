//! What a client build reads beyond the wire shape older builds check with
//! exact keys. Web clients are served from central and can be older than a
//! host, so a field newer than them is sent only to a client that advertised
//! it on the WebSocket URL query (old hosts ignore unknown pairs there).
use axum::extract::ws::Message;
use serde_json::Value;

/// Room settings may carry `agent_chain_limit`.
pub(crate) const AGENT_CHAIN_LIMIT_QUERY: &str = "agent_chain_limit";
/// The client reads `participant_activity` frames.
const PARTICIPANT_ACTIVITY_QUERY: &str = "participant_activity";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ClientWire {
    /// The optional `channel_message_cursors` snapshot field.
    pub(crate) channel_message_cursors: bool,
    /// `agent_chain_limit` in room settings, wherever they appear.
    pub(crate) agent_chain_limit: bool,
    /// Which external AIs are working, as `participant_activity` frames.
    pub(crate) participant_activity: bool,
}

impl ClientWire {
    pub(crate) fn from_query(query: Option<&str>) -> Self {
        Self {
            channel_message_cursors: crate::room_socket::client_advertises_channel_message_cursors(
                query,
            ),
            agent_chain_limit: advertises(query, AGENT_CHAIN_LIMIT_QUERY),
            participant_activity: advertises(query, PARTICIPANT_ACTIVITY_QUERY),
        }
    }

    /// The frame as this client can read it: room settings lose
    /// `agent_chain_limit` unless the client advertised it.
    pub(crate) fn adapt(self, message: Message) -> Message {
        let Message::Text(text) = &message else {
            return message;
        };
        if self.agent_chain_limit || !text.as_str().contains("\"agent_chain_limit\"") {
            return message;
        }
        let Ok(mut value) = serde_json::from_str::<Value>(text.as_str()) else {
            return message;
        };
        drop_agent_chain_limit(&mut value);
        serde_json::to_string(&value).map_or(message, |text| Message::Text(text.into()))
    }
}

fn advertises(query: Option<&str>, name: &str) -> bool {
    query.is_some_and(|query| {
        query
            .split('&')
            .any(|pair| pair.split_once('=') == Some((name, "1")))
    })
}

/// Room settings are the objects that carry `settings_revision`.
fn drop_agent_chain_limit(value: &mut Value) {
    match value {
        Value::Object(map) => {
            if map.contains_key("settings_revision") {
                map.remove(AGENT_CHAIN_LIMIT_QUERY);
            }
            map.values_mut().for_each(drop_agent_chain_limit);
        }
        Value::Array(items) => items.iter_mut().for_each(drop_agent_chain_limit),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use axum::extract::ws::Message;
    use serde_json::{Value, json};

    use super::ClientWire;

    fn sent(wire: ClientWire, frame: &Value) -> Value {
        match wire.adapt(Message::Text(frame.to_string().into())) {
            Message::Text(text) => serde_json::from_str(text.as_str()).unwrap_or_default(),
            _ => Value::Null,
        }
    }

    #[test]
    fn only_an_advertising_client_reads_the_agent_chain_limit() {
        let frame = json!({"op": "snapshot", "room_settings": {"settings_revision": "r", "agent_chain_limit": 0},
            "events": [{"type": "room_settings_updated", "room_settings": {"settings_revision": "r", "agent_chain_limit": 50}}],
            "content": {"agent_chain_limit": "a message that only mentions it"}});
        let old = sent(
            ClientWire::from_query(Some("ticket=t&channel_message_cursors=1")),
            &frame,
        );
        assert!(old["room_settings"].get("agent_chain_limit").is_none());
        assert!(
            old["events"][0]["room_settings"]
                .get("agent_chain_limit")
                .is_none()
        );
        assert_eq!(
            old["content"]["agent_chain_limit"],
            frame["content"]["agent_chain_limit"]
        );
        let new = sent(
            ClientWire::from_query(Some("ticket=t&agent_chain_limit=1")),
            &frame,
        );
        assert_eq!(new, frame);
    }

    #[test]
    fn only_an_advertising_client_gets_activity_frames() {
        assert!(!ClientWire::from_query(Some("ticket=t&agent_chain_limit=1")).participant_activity);
        assert!(!ClientWire::from_query(Some("participant_activity=0")).participant_activity);
        assert!(
            ClientWire::from_query(Some("ticket=t&participant_activity=1")).participant_activity
        );
    }
}
