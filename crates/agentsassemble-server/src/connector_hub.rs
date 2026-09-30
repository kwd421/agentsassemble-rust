use std::{collections::HashMap, sync::Arc};

use agentsassemble_protocol::{CommandResolution, RoomAction};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use parking_lot::Mutex;
use serde_json::{Value, json};
use url::Url;

use crate::connector_client::{RoomConnectorClient, transport::normalize_server};

pub(super) struct ConnectorHub {
    allowed_servers: Option<Vec<Url>>,
    owned_runtime: Option<HostedRuntime>,
    state: Mutex<HubState>,
}
struct HostedRuntime {
    ingress: crate::public_ingress::PublicIngress,
    store: agentsassemble_persistence::SqliteStore,
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
            owned_runtime: None,
            state: Mutex::new(HubState::default()),
        })
    }

    pub(super) fn hosted(
        ingress: crate::public_ingress::PublicIngress,
        store: agentsassemble_persistence::SqliteStore,
    ) -> Self {
        Self {
            allowed_servers: None,
            owned_runtime: Some(HostedRuntime { ingress, store }),
            state: Mutex::new(HubState::default()),
        }
    }

    fn remote(&self) -> bool {
        self.allowed_servers.is_some() || self.owned_runtime.is_some()
    }

    pub(super) fn owns_runtime(&self) -> bool {
        self.owned_runtime.is_some()
    }

    fn candidate(&self, invite: &str, name: &str) -> Result<RoomConnectorClient, String> {
        let Some(runtime) = self.owned_runtime.as_ref() else {
            return RoomConnectorClient::new(invite, name, self.allowed_servers.as_deref())
                .map_err(|error| error.code);
        };
        let local = runtime
            .ingress
            .local_url()
            .ok_or_else(|| "local_ingress_unavailable".to_owned())?;
        let local = normalize_server(&local).map_err(|error| error.code)?;
        let mut allowed = vec![local.clone()];
        if let Some(ready) = runtime.ingress.ready_snapshot() {
            allowed.push(normalize_server(&ready.public_url).map_err(|error| error.code)?);
        }
        RoomConnectorClient::new_hosted(invite, name, &allowed, local).map_err(|error| error.code)
    }

    pub(super) async fn join(&self, invite: &str, name: &str, id: &str) -> Result<Value, String> {
        let candidate = self.candidate(invite, name)?;
        if id.is_empty()
            && let Some(runtime) = &self.owned_runtime
        {
            // This public preparation step retains capacity. Require evidence of
            // an issued capability before reserving it; admission still validates
            // scope, expiry, revocation and retry identity in its transaction.
            let (_, bearer) = crate::connector_client::transport::parse_invite(invite)
                .map_err(|error| error.code)?;
            let fingerprint = crate::http_api::purpose_credential_fingerprint(
                &bearer,
                agentsassemble_persistence::CONNECTOR_INVITE_PREFIX,
            )
            .ok_or_else(|| "invite_unavailable".to_owned())?;
            if runtime
                .store
                .connector_admission_room_id(&fingerprint)
                .await
                .map_err(|_| "connector_invite_lookup_failed".to_owned())?
                .is_none()
            {
                return Err("invite_unavailable".to_owned());
            }
        }
        let (id, client) = if id.is_empty() {
            let reserved = self.reserve(candidate)?;
            // Remote HTTP is stateless. Establish private retry custody before any
            // admission effect; an invitation and public name cannot select a client.
            if self.remote() {
                return Ok(json!({
                    "status": "connection_prepared", "connection_id": reserved.0,
                    "instructions": "Not in the room yet. Calling room_join again with this connection_id and the same invite_url and display_name enters it; the same ID works after a failed response. connection_id is private to this connection."
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
                "instructions": "Joined as this conversation. room_read shows the room, room_say posts, room_wait_next waits for others. connection_id is private and is passed unchanged to later tools."
            })),
            Err(error) => {
                if error.resolution == Some(CommandResolution::Rejected)
                    && client.close_rejected_admission().await
                {
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
        if !self.remote()
            && let Some((id, client)) = state.clients.iter().find(|(_, client)| {
                !client.has_completed_leave()
                    && client.invitation_identity() == candidate.invitation_identity()
            })
        {
            if client.display_name != candidate.display_name {
                return Err("connector_join_name_conflict".to_owned());
            }
            return Ok((id.clone(), client.clone()));
        }
        if state.clients.len() >= 128 {
            return Err("connector_capacity_release_receipt_required".to_owned());
        }
        if !self.remote()
            && state
                .clients
                .values()
                .any(|client| !client.has_completed_leave())
        {
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
        let client = self.connection(id, false)?;
        if client.has_completed_leave() {
            return Err("invalid_connection_id".to_owned());
        }
        Ok(client)
    }

    fn connection(&self, id: &str, for_leave: bool) -> Result<Arc<RoomConnectorClient>, String> {
        let state = self.state.lock();
        if state.closed {
            return Err("connector_closed".to_owned());
        }
        if id.is_empty() && !self.remote() {
            if for_leave && state.clients.len() > 1 {
                return Err("connection_id_required".to_owned());
            }
            if let Some(client) = state
                .clients
                .values()
                .find(|client| !client.has_completed_leave())
            {
                return Ok(client.clone());
            }
            return match state.clients.len() {
                0 => Err("connector_not_joined".to_owned()),
                1 => state
                    .clients
                    .values()
                    .next()
                    .cloned()
                    .ok_or_else(|| "connector_not_joined".to_owned()),
                _ => Err("connection_id_required".to_owned()),
            };
        }
        state
            .clients
            .get(id)
            .cloned()
            .ok_or_else(|| "invalid_connection_id".to_owned())
    }

    pub(super) async fn leave(&self, id: &str, release_receipt: bool) -> Result<Value, String> {
        let client = self.connection(id, true)?;
        if release_receipt {
            if !client.has_completed_leave() {
                return Err("connector_leave_receipt_not_completed".to_owned());
            }
            self.remove(id, &client);
            return Ok(json!({"status":"receipt_released"}));
        }
        if client.cancel_prepared().await {
            self.remove(id, &client);
            return Ok(json!({"status":"connection_cancelled"}));
        }
        let result = client
            .command(RoomAction::ParticipantLeave, json!({}))
            .await
            .map_err(|error| error.code)?;
        Ok(result)
    }

    fn remove(&self, id: &str, client: &Arc<RoomConnectorClient>) {
        let mut state = self.state.lock();
        let key = if id.is_empty() && !self.remote() {
            state
                .clients
                .iter()
                .find(|(_, current)| Arc::ptr_eq(current, client))
                .map(|(key, _)| key.clone())
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
