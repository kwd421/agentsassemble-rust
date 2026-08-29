use agentsassemble_domain::{is_message_attachment_id, validate_room_id};
use tauri::AppHandle;

use super::{
    HttpTicketGrant, LocalRuntime,
    control::{request_message_attachment_read_ticket, request_message_attachment_upload_ticket},
    ensure_runtime, handle_ticket_result,
};

impl LocalRuntime {
    /// Issues a message-attachment upload ticket for one validated room.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid input, rejected write authority, or a broken owned runtime.
    pub fn issue_message_attachment_upload_ticket(
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
            request_message_attachment_upload_ticket(ensure_runtime(&mut process, app)?, &room_id);
        handle_ticket_result(&mut process, result)
    }

    /// Issues an exact asset-bound message-attachment read ticket for one validated room.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid input, rejected read authority, or a broken owned runtime.
    pub fn issue_message_attachment_read_ticket(
        &self,
        app: &AppHandle,
        requested_room_id: &str,
        attachment_id: &str,
    ) -> Result<HttpTicketGrant, String> {
        let room_id = validate_room_id(requested_room_id)
            .map_err(|error| format!("invalid room id: {}", error.message))?;
        if !is_message_attachment_id(attachment_id) {
            return Err("invalid message attachment id".to_owned());
        }
        let mut process = self
            .process
            .lock()
            .map_err(|_| "local runtime state lock is poisoned".to_owned())?;
        let result = request_message_attachment_read_ticket(
            ensure_runtime(&mut process, app)?,
            &room_id,
            attachment_id,
        );
        handle_ticket_result(&mut process, result)
    }
}
