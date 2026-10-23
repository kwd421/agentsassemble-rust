use std::{collections::HashMap, sync::Arc};

use agentsassemble_protocol::{CommandResolution, RoomAction};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use parking_lot::Mutex;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use url::Url;

use crate::connector::client::{RoomConnectorClient, transport::normalize_server};

pub(super) struct ConnectorHub {
    allowed_servers: Option<Vec<Url>>,
    owned_runtime: Option<crate::AppState>,
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
            owned_runtime: None,
            state: Mutex::new(HubState::default()),
        })
    }

    pub(super) fn hosted(state: crate::AppState) -> Self {
        Self {
            allowed_servers: None,
            owned_runtime: Some(state),
            state: Mutex::new(HubState::default()),
        }
    }

    pub(super) async fn upload(
        &self,
        input: super::contract::AttachmentUpload,
        admission: Option<&crate::connector::mcp_web::RequestAdmission>,
    ) -> Result<Value, String> {
        let client = self.client_or_resume(&input.connection_id).await?;
        if let Some(state) = &self.owned_runtime {
            let fingerprint = client
                .session_fingerprint()
                .await
                .map_err(|error| error.code)?;
            let upload = crate::connector::web::attachments::upload_parsed(
                state.clone(),
                fingerprint,
                crate::profile_web::EncodedAttachmentUpload {
                    filename: input.filename,
                    content_type: input.content_type,
                    data_base64: input.data_base64,
                },
                admission,
            );
            // Preserve the connector client's close and uncertain-timeout semantics
            // while storage retains its own lease if it has already started.
            return tokio::select! {
                () = client.cancellation.cancelled() => Err("connector_closed".into()),
                result = tokio::time::timeout(crate::http_body_policy::UPLOAD_CEILING, upload) => {
                    result.map_err(|_| "connector_transport_unresolved".to_owned())?
                        .map(|value| value.0).map_err(|error| error.code.into_owned())
                }
            };
        }
        // Standalone HTTP/stdio have no host store or shared host upload pool.
        // Keep their envelope until the authenticated remote upload completes.
        client
            .upload_attachment(&input.filename, &input.content_type, &input.data_base64)
            .await
            .map_err(|error| error.code)
    }

    pub(super) async fn read_attachment(
        &self,
        input: super::contract::AttachmentRead,
        admission: Option<&crate::connector::mcp_web::RequestAdmission>,
    ) -> Result<agentsassemble_provider::ProviderAttachment, String> {
        let client = self.client_or_resume(&input.connection_id).await?;
        if let Some(state) = &self.owned_runtime {
            let admission = admission.ok_or("mcp_request_admission_required")?;
            let read = async {
                let fingerprint = client.session_fingerprint().await.map_err(|e| e.code)?;
                let authorization =
                    crate::connector::web::read::authorize_read_fingerprint(state, &fingerprint, 1)
                        .await
                        .map_err(|e| e.code.into_owned())?;
                let (attachment, lease) = crate::connector::web::attachments::read_authorized(
                    state,
                    &authorization,
                    &input.attachment_id,
                )
                .await
                .map_err(|e| e.code.into_owned())?;
                admission.retain_download(lease)?;
                Ok(attachment)
            };
            return tokio::select! {
                () = client.cancellation.cancelled() => Err("connector_closed".into()),
                result = tokio::time::timeout(crate::connector::client::COMMAND_TIMEOUT, read) => {
                    result.map_err(|_| "connector_transport_unresolved".to_owned())?
                }
            };
        }
        client
            .read_attachment(&input.attachment_id)
            .await
            .map_err(|e| e.to_string())
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
            .public_ingress()
            .local_url()
            .ok_or_else(|| "local_ingress_unavailable".to_owned())?;
        let local = normalize_server(&local).map_err(|error| error.code)?;
        let mut allowed = vec![local.clone()];
        if let Some(ready) = runtime.public_ingress().ready_snapshot() {
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
            let (_, bearer) = crate::connector::client::transport::parse_invite(invite)
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
            let client = self.client_or_resume(id).await?;
            if client.invitation_identity() != candidate.invitation_identity()
                || client.display_name != candidate.display_name
            {
                return Err("connector_join_identity_conflict".to_owned());
            }
            (id.to_owned(), client)
        };
        match client.join().await {
            Ok(joined) => {
                self.remember(&id, &client).await;
                Ok(json!({
                "status": "joined", "connection_id": id,
                "room_id": joined.room_id, "room_uid": joined.room_uid,
                "conversation_mode": joined.room_state["conversation_mode"], "guidance": joined.room_state["guidance"], "unread_mentions": joined.room_state["unread_mentions"], "mention_overflow": joined.room_state["mention_overflow"], "mention_resync_guidance": joined.room_state["mention_resync_guidance"],
                "participant_id": joined.participant_id, "display_name": joined.display_name,
                "role": joined.room_state["role"], "delegated_actions": joined.room_state["delegated_actions"],
                "your_instructions": joined.room_state["your_instructions"],
                "instructions": "Joined as this conversation. room_read shows the room, room_say posts, room_wait_next follows the current conversation mode and guidance. connection_id is private and is passed unchanged to later tools."
                }))
            }
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

    /// Like `client`, but after a host restart a hosted connection the AI still
    /// holds the ID of is restored into its remembered seat.
    pub(super) async fn client_or_resume(
        &self,
        id: &str,
    ) -> Result<Arc<RoomConnectorClient>, String> {
        let missing = match self.client(id) {
            Err(code) if code == "invalid_connection_id" && !id.is_empty() => code,
            found => return found,
        };
        let Some(runtime) = &self.owned_runtime else {
            return Err(missing);
        };
        // Issued IDs are 32 random bytes in unpadded base64url; nothing else is looked up.
        if id.len() != 43
            || !id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
        {
            return Err(missing);
        }
        let hash: [u8; 32] = Sha256::digest(id.as_bytes()).into();
        let Some(resumed) = runtime
            .store
            .resume_connector_connection(&hash, chrono::Utc::now())
            .await
            .map_err(|_| "connector_resume_failed".to_owned())?
        else {
            return Err(missing);
        };
        let local = runtime
            .public_ingress()
            .local_url()
            .ok_or_else(|| "local_ingress_unavailable".to_owned())?;
        let local = normalize_server(&local).map_err(|error| error.code)?;
        if let Ok(client) = self.client(id) {
            return Ok(client);
        }
        let client =
            Arc::new(RoomConnectorClient::resumed(local, resumed).map_err(|error| error.code)?);
        client.join().await.map_err(|error| error.code)?;
        let mut state = self.state.lock();
        if state.closed {
            return Err("connector_closed".to_owned());
        }
        if !state.clients.contains_key(id) && state.clients.len() >= 128 {
            return Err("connector_capacity_release_receipt_required".to_owned());
        }
        Ok(state.clients.entry(id.to_owned()).or_insert(client).clone())
    }

    /// Lets a hosted connection find its seat again after a host restart.
    async fn remember(&self, id: &str, client: &RoomConnectorClient) {
        let Some(runtime) = &self.owned_runtime else {
            return;
        };
        let Ok(fingerprint) = client.session_fingerprint().await else {
            return;
        };
        let hash: [u8; 32] = Sha256::digest(id.as_bytes()).into();
        if let Err(error) = runtime
            .store
            .remember_connector_connection(
                &hash,
                &fingerprint,
                &client.invitation_identity(),
                &client.display_name,
                chrono::Utc::now(),
            )
            .await
        {
            tracing::warn!(code = %crate::room_command_execution::persistence_error_code(&error), "connector connection is not remembered across a restart");
        }
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
        // After a host restart the connection is restored first, so it can leave.
        if !id.is_empty() && self.connection(id, true).is_err() {
            let _ = self.client_or_resume(id).await;
        }
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
