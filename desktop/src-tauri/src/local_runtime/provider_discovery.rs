use agentsassemble_protocol::{LocalControlRequest, LocalControlResponse};
use tauri::AppHandle;
use uuid::Uuid;

use super::{
    LocalRuntime,
    control::{TicketFailure, request_control},
    ensure_runtime, handle_ticket_result,
};

impl LocalRuntime {
    pub(crate) fn discover_provider(
        &self,
        app: &AppHandle,
        provider_id: &str,
        force: bool,
    ) -> Result<u64, String> {
        let mut process = self
            .process
            .lock()
            .map_err(|_| "local runtime state lock is poisoned".to_owned())?;
        let request_id = Uuid::new_v4().to_string();
        let result = request_control(
            ensure_runtime(&mut process, app)?,
            &LocalControlRequest::DiscoverLocalProvider {
                request_id: request_id.clone(),
                provider_id: provider_id.to_owned(),
                force,
            },
        )
        .and_then(|response| match response {
            LocalControlResponse::ProviderDiscoveryOk {
                request_id: response_id,
                provider_id: response_provider,
                generation,
            } if response_id == request_id && response_provider == provider_id => Ok(generation),
            LocalControlResponse::Error {
                request_id: response_id,
                message,
                ..
            } if response_id == request_id => Err(TicketFailure::Rejected(message)),
            _ => Err(TicketFailure::Broken(
                "Local provider discovery response did not match its request.".into(),
            )),
        });
        handle_ticket_result(&mut process, result)
    }
}
