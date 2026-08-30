use agentsassemble_domain::validate_room_id;
use tauri::AppHandle;

use super::{
    HttpTicketGrant, LocalRuntime, control::request_message_search_read_ticket, ensure_runtime,
    handle_ticket_result,
};

impl LocalRuntime {
    /// Issues a message-search read-only HTTP ticket for one validated room.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid room, rejected identity, or broken owned runtime.
    pub fn issue_message_search_read_ticket(
        &self,
        app: &AppHandle,
        requested_room_id: &str,
    ) -> Result<HttpTicketGrant, String> {
        let room_id = validate_room_id(requested_room_id)
            .map_err(|error| format!("invalid room id: {}", error.message))?;
        let mut process = self
            .process
            .lock()
            .map_err(|_| "local runtime state lock is poisoned".to_owned())?;
        let result =
            request_message_search_read_ticket(ensure_runtime(&mut process, app)?, &room_id);
        handle_ticket_result(&mut process, result)
    }
}
