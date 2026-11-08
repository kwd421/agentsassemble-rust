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
/// The client reads `listening` in those frames.
const PARTICIPANT_LISTENING_QUERY: &str = "participant_listening";
/// The client reads delegated AI invite requests.
const DELEGATED_AI_INVITE_QUERY: &str = "delegated_ai_invite";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ClientWire {
    /// The optional `channel_message_cursors` snapshot field.
    pub(crate) channel_message_cursors: bool,
    /// `agent_chain_limit` in room settings, wherever they appear.
    pub(crate) agent_chain_limit: bool,
    /// Which `participant_activity` frames the client reads.
    pub(crate) participant_activity: ActivityWire,
    /// Delegated AI invite requests (`invitee` in invite params).
    pub(crate) delegated_ai_invite: bool,
}

/// How much external-AI activity a client reads.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum ActivityWire {
    #[default]
    Off,
    /// Which external AIs are working.
    Working,
    /// Also which have a wait pending (`listening`).
    Listening,
}

impl ClientWire {
    pub(crate) fn from_query(query: Option<&str>) -> Self {
        Self {
            channel_message_cursors: crate::room_socket::client_advertises_channel_message_cursors(
                query,
            ),
            agent_chain_limit: advertises(query, AGENT_CHAIN_LIMIT_QUERY),
            participant_activity: match (
                advertises(query, PARTICIPANT_ACTIVITY_QUERY),
                advertises(query, PARTICIPANT_LISTENING_QUERY),
            ) {
                (false, _) => ActivityWire::Off,
                (true, false) => ActivityWire::Working,
                (true, true) => ActivityWire::Listening,
            },
            delegated_ai_invite: advertises(query, DELEGATED_AI_INVITE_QUERY),
        }
    }

    /// The frame as this client can read it: room settings lose
    /// `agent_chain_limit`, and AI invite requests are hidden, unless the
    /// client advertised them.
    pub(crate) fn adapt(self, message: Message) -> Message {
        let Message::Text(text) = &message else {
            return message;
        };
        let chain_limit =
            !self.agent_chain_limit && text.as_str().contains("\"agent_chain_limit\"");
        let ai_invite = !self.delegated_ai_invite && text.as_str().contains("\"invitee\"");
        if !chain_limit && !ai_invite {
            return message;
        }
        let Ok(mut value) = serde_json::from_str::<Value>(text.as_str()) else {
            return message;
        };
        if chain_limit {
            drop_agent_chain_limit(&mut value);
        }
        if ai_invite {
            hide_ai_invites(&mut value);
        }
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

fn is_ai_invite(request: &Value) -> bool {
    request
        .pointer("/action/params/invitee")
        .is_some_and(|invitee| invitee != "human")
}

/// An older client rejects the `invitee` key: pending lists drop AI invite
/// requests, and their events arrive as already cancelled, keeping `seq` whole.
fn hide_ai_invites(value: &mut Value) {
    match value {
        Value::Object(map) => {
            if let Some(Value::Array(requests)) = map.get_mut("delegated_action_requests") {
                requests.retain(|request| !is_ai_invite(request));
            }
            if let Some(request) = map
                .get_mut("delegated_action_request")
                .filter(|request| is_ai_invite(request))
            {
                if let Some(Value::Object(params)) = request.pointer_mut("/action/params") {
                    params.remove("invitee");
                }
                request["state"] = Value::from("cancelled");
            }
            map.values_mut().for_each(hide_ai_invites);
        }
        Value::Array(items) => items.iter_mut().for_each(hide_ai_invites),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use axum::extract::ws::Message;
    use serde_json::{Value, json};

    use super::{ActivityWire, ClientWire};

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
    fn an_older_client_never_sees_an_ai_invite_request() {
        let ai = json!({"confirmation_id": "c", "state": "pending",
            "action": {"action": "create_human_invite", "params": {"request_id": "r", "invitee": "ai"}}});
        let person = json!({"confirmation_id": "p", "state": "pending",
            "action": {"action": "create_human_invite", "params": {"request_id": "q"}}});
        let frame = json!({"op": "snapshot", "delegated_action_requests": [ai, person],
            "events": [{"type": "delegated_action_requested", "seq": 4, "delegated_action_request": ai}]});
        let old = sent(ClientWire::from_query(Some("ticket=t")), &frame);
        assert_eq!(old["delegated_action_requests"], json!([person]));
        let event = &old["events"][0];
        assert_eq!(event["seq"], 4);
        assert_eq!(event["delegated_action_request"]["state"], "cancelled");
        assert!(
            event["delegated_action_request"]["action"]["params"]
                .get("invitee")
                .is_none()
        );
        let new = sent(
            ClientWire::from_query(Some("ticket=t&delegated_ai_invite=1")),
            &frame,
        );
        assert_eq!(new, frame);
    }

    #[test]
    fn only_an_advertising_client_gets_activity_frames() {
        let wire = |query| ClientWire::from_query(Some(query)).participant_activity;
        assert_eq!(wire("ticket=t&agent_chain_limit=1"), ActivityWire::Off);
        assert_eq!(wire("participant_activity=0"), ActivityWire::Off);
        assert_eq!(wire("participant_listening=1"), ActivityWire::Off);
        assert_eq!(
            wire("ticket=t&participant_activity=1"),
            ActivityWire::Working
        );
        assert_eq!(
            wire("participant_activity=1&participant_listening=1"),
            ActivityWire::Listening
        );
    }
}
