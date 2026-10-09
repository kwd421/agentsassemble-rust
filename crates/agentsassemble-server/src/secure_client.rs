//! Authority carried only by the authenticated encrypted transport, never headers.
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use agentsassemble_persistence::{
    OwnerSessionAuthorization, PersistenceError, SecureSessionBinding,
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use parking_lot::Mutex;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;

use crate::{
    AppState, central::host_identity::SecureClientHello, owner_session_lifetime::OwnerSessionLease,
};

#[derive(Clone)]
pub(crate) struct SecureClient(Arc<Custody>);

struct Custody {
    hello: SecureClientHello,
    binding: SecureSessionBinding,
    admitted: AtomicBool,
    owner: Mutex<Option<OwnerSessionLease>>,
    closed: CancellationToken,
    adopted: tokio::sync::Notify,
}

impl SecureClient {
    pub(crate) fn new(hello: SecureClientHello, channel_id: String) -> Result<Self, ()> {
        let key = URL_SAFE_NO_PAD
            .decode(&hello.client_public_key)
            .map_err(|_| ())?;
        Ok(Self(Arc::new(Custody {
            hello,
            binding: SecureSessionBinding {
                client_key_fingerprint: Sha256::digest(key).into(),
                channel_id,
            },
            admitted: AtomicBool::new(false),
            owner: Mutex::new(None),
            closed: CancellationToken::new(),
            adopted: tokio::sync::Notify::new(),
        })))
    }

    pub(crate) fn hello(&self) -> &SecureClientHello {
        &self.0.hello
    }
    pub(crate) fn binding(&self) -> &SecureSessionBinding {
        &self.0.binding
    }
    pub(crate) fn closed(&self) -> &CancellationToken {
        &self.0.closed
    }
    pub(crate) fn admitted(&self) -> bool {
        self.0.admitted.load(Ordering::Acquire)
    }

    pub(crate) fn close(&self) {
        let mut owner = self.0.owner.lock();
        self.0.closed.cancel();
        owner.take();
    }

    pub(crate) fn adopt(
        &self,
        state: &AppState,
        owner: Option<&OwnerSessionAuthorization>,
    ) -> Result<(), PersistenceError> {
        let mut lease = self.0.owner.lock();
        if self.0.closed.is_cancelled() {
            return Err(PersistenceError::CommandRejected {
                code: "secure_channel_closed".into(),
                message: "The server connection has ended.".into(),
            });
        }
        if lease.is_none()
            && let Some(owner) = owner
        {
            *lease = Some(state.owner_sessions.retain(owner)?);
        }
        self.0.admitted.store(true, Ordering::Release);
        self.0.adopted.notify_one();
        Ok(())
    }

    pub(crate) async fn owner_ended(&self) {
        loop {
            let changed = self.0.adopted.notified();
            let status = self
                .0
                .owner
                .lock()
                .as_ref()
                .map(|lease| lease.status.clone());
            if let Some(mut status) = status {
                let _ = status
                    .wait_for(|value| {
                        matches!(
                            value,
                            agentsassemble_protocol::CentralOwnerSessionStatus::Ended { .. }
                        )
                    })
                    .await;
                return;
            }
            changed.await;
        }
    }

    pub(crate) fn add_redeem_fields(&self, body: &mut Value, purpose: &str) -> Result<(), ()> {
        let expected = match self.hello().purpose.as_str() {
            "owner" => "owner",
            "member_admission" => "admission",
            "member_connect" => "connect",
            "account_deletion" => "account_deletion",
            _ => return Err(()),
        };
        if expected != purpose || self.closed().is_cancelled() {
            return Err(());
        }
        let fields = json!({"protocol": self.hello().protocol, "registration_epoch": self.hello().registration_epoch,
            "origin": self.hello().origin, "generation": self.hello().generation,
            "client_public_key": self.hello().client_public_key, "channel_id": self.binding().channel_id, "purpose": purpose});
        body.as_object_mut()
            .ok_or(())?
            .extend(fields.as_object().ok_or(())?.clone());
        Ok(())
    }

    pub(crate) fn verify_echo(&self, response: &Value, purpose: &str) -> Result<(), ()> {
        if response["protocol"] != self.hello().protocol
            || response["client_public_key"] != self.hello().client_public_key
            || response["channel_id"] != self.binding().channel_id
            || response["purpose"] != purpose
        {
            return Err(());
        }
        Ok(())
    }
}

pub(crate) fn from_request(request: &axum::extract::Request) -> Option<SecureClient> {
    request.extensions().get::<SecureClient>().cloned()
}
