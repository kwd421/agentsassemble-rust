use super::{
    CentralDirectory, CentralDirectoryError, CentralHostIdentity, Deserialize, Digest, Method,
    Serialize, SqliteStore, json, send_signed,
};

#[derive(Clone, Copy)]
pub(crate) struct RedeemHost<'a> {
    pub identity: &'a CentralHostIdentity,
    pub store: &'a SqliteStore,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OwnerAdmissionResponse {
    status: String,
    server_id: String,
    person_id: String,
    device_id: String,
    origin: String,
    generation: i64,
    expires_at: i64,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MemberGrantPurpose {
    Admission,
    Connect,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MemberAdmissionResponse {
    pub(crate) projection_id: String,
    pub(crate) person_id: String,
    pub(crate) issuer: String,
    pub(crate) display_name: String,
}

impl CentralDirectory {
    pub(crate) async fn member_admission(
        &self,
        host: RedeemHost<'_>,
        grant: &str,
        challenge_hash: &str,
        epoch: &str,
        purpose: MemberGrantPurpose,
        secure: Option<&crate::secure_client::SecureClient>,
    ) -> Result<MemberAdmissionResponse, CentralDirectoryError> {
        let RedeemHost { identity, store } = host;
        let inner = self.0.as_ref().ok_or(CentralDirectoryError::Disabled)?;
        let (prefix, route) = match purpose {
            MemberGrantPurpose::Admission => ("aamg1.", "member-grants"),
            MemberGrantPurpose::Connect => ("aamc1.", "member-connect-grants"),
        };
        if !grant.starts_with(prefix)
            || grant.len() > 256
            || store.registration_epoch().await?.as_deref() != Some(epoch)
        {
            return Err(CentralDirectoryError::Rejected);
        }
        let path = format!("/v1/servers/{}/{route}/redeem", identity.server_id());
        let mut body = json!({"grant_token": grant, "challenge_hash": challenge_hash,
            "registration_epoch": epoch, "purpose": purpose});
        if let Some(secure) = secure {
            secure
                .add_redeem_fields(
                    &mut body,
                    match purpose {
                        MemberGrantPurpose::Admission => "admission",
                        MemberGrantPurpose::Connect => "connect",
                    },
                )
                .map_err(|()| CentralDirectoryError::Rejected)?;
        }
        let bytes = send_signed(inner, identity, store, Method::POST, &path, body).await?;
        let mut value: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|_| CentralDirectoryError::InvalidResponse)?;
        if let Some(secure) = secure {
            secure
                .verify_echo(
                    &value,
                    match purpose {
                        MemberGrantPurpose::Admission => "admission",
                        MemberGrantPurpose::Connect => "connect",
                    },
                )
                .map_err(|()| CentralDirectoryError::InvalidResponse)?;
            for field in ["protocol", "client_public_key", "channel_id", "purpose"] {
                value
                    .as_object_mut()
                    .ok_or(CentralDirectoryError::InvalidResponse)?
                    .remove(field);
            }
        }
        let response: MemberAdmissionResponse =
            serde_json::from_value(value).map_err(|_| CentralDirectoryError::InvalidResponse)?;
        if base64::Engine::decode(
            &base64::engine::general_purpose::URL_SAFE_NO_PAD,
            &response.projection_id,
        )
        .map_or(true, |id| id.len() != 16)
            || response.issuer != inner.base_url.origin().ascii_serialization()
            || response.person_id.is_empty()
            || response.person_id.len() > 256
            || response.display_name.trim().is_empty()
            || response.display_name.chars().count() > 80
        {
            return Err(CentralDirectoryError::InvalidResponse);
        }
        Ok(response)
    }

    pub(crate) async fn owner_admission(
        &self,
        host: RedeemHost<'_>,
        credential: &str,
        origin: &str,
        generation: i64,
        device: &[u8; 32],
        secure: Option<&crate::secure_client::SecureClient>,
    ) -> Result<agentsassemble_persistence::OwnerAdmission, CentralDirectoryError> {
        let RedeemHost { identity, store } = host;
        let inner = self.0.as_ref().ok_or(CentralDirectoryError::Disabled)?;
        let path = format!("/v1/servers/{}/connect-grants/redeem", identity.server_id());
        let mut body = json!({ "grant_token": credential, "origin": origin,
            "generation": generation });
        if let Some(secure) = secure {
            secure
                .add_redeem_fields(&mut body, "owner")
                .map_err(|()| CentralDirectoryError::Rejected)?;
        }
        let bytes = send_signed(inner, identity, store, Method::POST, &path, body).await?;
        let mut value: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|_| CentralDirectoryError::InvalidResponse)?;
        if let Some(secure) = secure {
            secure
                .verify_echo(&value, "owner")
                .map_err(|()| CentralDirectoryError::InvalidResponse)?;
            if value["registration_epoch"] != secure.hello().registration_epoch {
                return Err(CentralDirectoryError::InvalidResponse);
            }
            value
                .as_object_mut()
                .ok_or(CentralDirectoryError::InvalidResponse)?
                .remove("registration_epoch");
            for field in ["protocol", "client_public_key", "channel_id", "purpose"] {
                value
                    .as_object_mut()
                    .ok_or(CentralDirectoryError::InvalidResponse)?
                    .remove(field);
            }
        }
        let response: OwnerAdmissionResponse =
            serde_json::from_value(value).map_err(|_| CentralDirectoryError::InvalidResponse)?;
        if response.status != "authorized"
            || response.server_id != identity.server_id()
            || response.origin != origin
            || response.generation != generation
        {
            return Err(CentralDirectoryError::InvalidResponse);
        }
        agentsassemble_persistence::OwnerAdmission::verified(
            agentsassemble_persistence::OwnerAdmissionBinding {
                secure: secure.map(|client| client.binding().clone()),
                entry_fingerprint: sha2::Sha256::digest(credential.as_bytes()).into(),
                server_id: response.server_id,
                person_id: response.person_id,
                device_id: response.device_id,
                browser_fingerprint: *device,
                origin: response.origin,
                generation: response.generation,
            },
            response.expires_at,
        )
        .map_err(|_| CentralDirectoryError::InvalidResponse)
    }
}
