use agentsassemble_domain::validate_room_id;
use tauri::AppHandle;

use super::{
    HttpTicketGrant, LocalRuntime,
    control::{request_human_invite_create_ticket, request_human_invite_revoke_ticket},
    ensure_runtime, handle_ticket_result,
};

#[derive(Clone, Copy)]
enum InviteTicketKind {
    Create,
    Revoke,
}

impl LocalRuntime {
    /// Issues an invite-create-only HTTP ticket for one validated room manager.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid room, rejected manager authority, or broken owned runtime.
    pub fn issue_human_invite_create_ticket(
        &self,
        app: &AppHandle,
        requested_room_id: &str,
    ) -> Result<HttpTicketGrant, String> {
        self.issue_human_invite_ticket(app, requested_room_id, InviteTicketKind::Create)
    }

    /// Issues an invite-revoke-only HTTP ticket for one validated room manager.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid room, rejected manager authority, or broken owned runtime.
    pub fn issue_human_invite_revoke_ticket(
        &self,
        app: &AppHandle,
        requested_room_id: &str,
    ) -> Result<HttpTicketGrant, String> {
        self.issue_human_invite_ticket(app, requested_room_id, InviteTicketKind::Revoke)
    }

    fn issue_human_invite_ticket(
        &self,
        app: &AppHandle,
        requested_room_id: &str,
        kind: InviteTicketKind,
    ) -> Result<HttpTicketGrant, String> {
        let room_id = validate_room_id(requested_room_id)
            .map_err(|error| format!("invalid room id: {}", error.message))?;
        let mut process = self
            .process
            .lock()
            .map_err(|_| "local runtime state lock is poisoned".to_owned())?;
        let runtime = ensure_runtime(&mut process, app)?;
        let result = match kind {
            InviteTicketKind::Create => request_human_invite_create_ticket(runtime, &room_id),
            InviteTicketKind::Revoke => request_human_invite_revoke_ticket(runtime, &room_id),
        };
        handle_ticket_result(&mut process, result)
    }
}
