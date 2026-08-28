use agentsassemble_domain::is_room_appearance_asset_id;
use tauri::AppHandle;

use super::{
    HttpTicketGrant, LocalRuntime, ManagerRoomAuthority,
    control::{
        request_appearance_bound_read_ticket, request_appearance_pending_read_ticket,
        request_appearance_upload_ticket,
    },
    ensure_runtime, handle_ticket_result,
    human_invite::validate_manager_room_authority,
};

#[derive(Clone, Copy)]
enum AppearanceTicketKind<'a> {
    Upload,
    PendingRead(&'a str),
    BoundRead(&'a str),
}

impl LocalRuntime {
    pub(crate) fn issue_appearance_upload_ticket(
        &self,
        app: &AppHandle,
        authority: ManagerRoomAuthority,
    ) -> Result<HttpTicketGrant, String> {
        self.issue_appearance_ticket(app, authority, AppearanceTicketKind::Upload)
    }

    pub(crate) fn issue_appearance_pending_read_ticket(
        &self,
        app: &AppHandle,
        authority: ManagerRoomAuthority,
        asset_id: &str,
    ) -> Result<HttpTicketGrant, String> {
        self.issue_appearance_ticket(app, authority, AppearanceTicketKind::PendingRead(asset_id))
    }

    pub(crate) fn issue_appearance_bound_read_ticket(
        &self,
        app: &AppHandle,
        authority: ManagerRoomAuthority,
        asset_id: &str,
    ) -> Result<HttpTicketGrant, String> {
        self.issue_appearance_ticket(app, authority, AppearanceTicketKind::BoundRead(asset_id))
    }

    fn issue_appearance_ticket(
        &self,
        app: &AppHandle,
        authority: ManagerRoomAuthority,
        kind: AppearanceTicketKind<'_>,
    ) -> Result<HttpTicketGrant, String> {
        let authority = validate_manager_room_authority(authority)?;
        if let AppearanceTicketKind::PendingRead(asset_id)
        | AppearanceTicketKind::BoundRead(asset_id) = kind
            && !is_room_appearance_asset_id(asset_id)
        {
            return Err("invalid room appearance asset id".to_owned());
        }
        let mut process = self
            .process
            .lock()
            .map_err(|_| "local runtime state lock is poisoned".to_owned())?;
        let runtime = ensure_runtime(&mut process, app)?;
        let result = match kind {
            AppearanceTicketKind::Upload => request_appearance_upload_ticket(runtime, &authority),
            AppearanceTicketKind::PendingRead(asset_id) => {
                request_appearance_pending_read_ticket(runtime, &authority, asset_id)
            }
            AppearanceTicketKind::BoundRead(asset_id) => {
                request_appearance_bound_read_ticket(runtime, &authority, asset_id)
            }
        };
        handle_ticket_result(&mut process, result)
    }
}
