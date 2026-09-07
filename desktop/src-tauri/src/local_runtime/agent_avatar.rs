use super::{
    HttpTicketGrant, LocalRuntime, ManagerRoomAuthority,
    control::request_agent_avatar_upload_ticket, ensure_runtime, handle_ticket_result,
    human_invite::validate_manager_room_authority,
};
use tauri::AppHandle;

impl LocalRuntime {
    pub(crate) fn issue_agent_avatar_upload_ticket(
        &self,
        app: &AppHandle,
        authority: ManagerRoomAuthority,
        session_id: &str,
    ) -> Result<HttpTicketGrant, String> {
        let authority = validate_manager_room_authority(authority)?;
        let mut process = self
            .process
            .lock()
            .map_err(|_| "local runtime state lock is poisoned".to_owned())?;
        let runtime = ensure_runtime(&mut process, app)?;
        let result = request_agent_avatar_upload_ticket(runtime, &authority, session_id);
        handle_ticket_result(&mut process, result)
    }
}
