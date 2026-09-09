use std::{collections::HashMap, sync::Arc};

use agentsassemble_protocol::{CommandResolution, RoomAction};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use parking_lot::Mutex;
use serde_json::{Value, json};
use url::Url;

use crate::connector_client::{RoomConnectorClient, transport::normalize_server};

pub(super) struct ConnectorHub {
    allowed_servers: Option<Vec<Url>>,
    state: Mutex<HubState>,
}
#[derive(Default)]
struct HubState {
    closed: bool,
    clients: HashMap<String, Arc<RoomConnectorClient>>,
}

impl ConnectorHub {
    pub(super) fn new(allowed_servers: Option<Vec<String>>) -> Result<Self, String> {
        let allowed_servers = allowed_servers
            .map(|servers| {
                if servers.is_empty() {
                    return Err("room_server_allowlist_required".to_owned());
                }
                servers
                    .iter()
                    .map(|server| normalize_server(server).map_err(|e| e.code))
                    .collect()
            })
            .transpose()?;
        Ok(Self {
            allowed_servers,
            state: Mutex::new(HubState::default()),
        })
    }

    pub(super) async fn join(&self, invite: &str, name: &str, id: &str) -> Result<Value, String> {
        let candidate = RoomConnectorClient::new(invite, name, self.allowed_servers.as_deref())
            .map_err(|error| error.code)?;
        let (id, client) = if id.is_empty() {
            let reserved = self.reserve(candidate)?;
            // Remote HTTP is stateless. Establish private retry custody before any
            // admission effect; an invitation and public name cannot select a client.
            if self.allowed_servers.is_some() {
                return Ok(json!({
                    "status": "connection_prepared", "connection_id": reserved.0,
                    "instructions": "No room admission has occurred. Keep connection_id private. Call room_join again with this exact connection_id and the same invite_url and display_name; retain that ID through any failed response."
                }));
            }
            reserved
        } else {
            let client = self.client(id)?;
            if client.invitation_identity() != candidate.invitation_identity()
                || client.display_name != candidate.display_name
            {
                return Err("connector_join_identity_conflict".to_owned());
            }
            (id.to_owned(), client)
        };
        match client.join().await {
            Ok(joined) => Ok(json!({
                "status": "joined", "connection_id": id,
                "room_id": joined.room_id, "room_uid": joined.room_uid,
                "participant_id": joined.participant_id, "display_name": joined.display_name,
                "instructions": "Immediately call room_read. Use room_say for substantive room contributions and room_wait_next to await others. Do not launch another model or delegate participation. Keep connection_id private and pass it unchanged to later tools."
            })),
            Err(error) => {
                if error.resolution == Some(CommandResolution::Rejected) {
                    self.remove(&id, &client);
                }
                Err(error.code)
            }
        }
    }

    fn reserve(
        &self,
        candidate: RoomConnectorClient,
    ) -> Result<(String, Arc<RoomConnectorClient>), String> {
        let mut state = self.state.lock();
        if state.closed {
            return Err("connector_closed".to_owned());
        }
        if self.allowed_servers.is_none()
            && let Some((id, client)) = state
                .clients
                .iter()
                .find(|(_, client)| client.invitation_identity() == candidate.invitation_identity())
        {
            if client.display_name != candidate.display_name {
                return Err("connector_join_name_conflict".to_owned());
            }
            return Ok((id.clone(), client.clone()));
        }
        let capacity = if self.allowed_servers.is_some() {
            128
        } else {
            1
        };
        if state.clients.len() >= capacity {
            return Err("connector_capacity_leave_required".to_owned());
        }
        let id = loop {
            let id = URL_SAFE_NO_PAD.encode(rand::random::<[u8; 32]>());
            if !state.clients.contains_key(&id) {
                break id;
            }
        };
        let client = Arc::new(candidate);
        state.clients.insert(id.clone(), client.clone());
        Ok((id, client))
    }

    pub(super) fn client(&self, id: &str) -> Result<Arc<RoomConnectorClient>, String> {
        let state = self.state.lock();
        if state.closed {
            return Err("connector_closed".to_owned());
        }
        if id.is_empty() && self.allowed_servers.is_none() {
            return state
                .clients
                .values()
                .next()
                .cloned()
                .ok_or_else(|| "connector_not_joined".to_owned());
        }
        state
            .clients
            .get(id)
            .cloned()
            .ok_or_else(|| "invalid_connection_id".to_owned())
    }

    pub(super) async fn leave(&self, id: &str) -> Result<Value, String> {
        let client = self.client(id)?;
        if client.cancel_prepared().await {
            self.remove(id, &client);
            return Ok(json!({"status":"connection_cancelled"}));
        }
        let result = client
            .command(RoomAction::ParticipantLeave, json!({}))
            .await
            .map_err(|error| error.code)?;
        self.remove(id, &client);
        Ok(result)
    }

    fn remove(&self, id: &str, client: &Arc<RoomConnectorClient>) {
        let mut state = self.state.lock();
        let key = if id.is_empty() && self.allowed_servers.is_none() {
            state.clients.keys().next().cloned()
        } else {
            Some(id.to_owned())
        };
        if let Some(key) = key
            && state
                .clients
                .get(&key)
                .is_some_and(|current| Arc::ptr_eq(current, client))
        {
            state.clients.remove(&key);
            client.close();
        }
    }

    pub(super) fn close(&self) {
        let mut state = self.state.lock();
        state.closed = true;
        for (_, client) in state.clients.drain() {
            client.close();
        }
    }
}

impl Drop for ConnectorHub {
    fn drop(&mut self) {
        self.close();
    }
}
