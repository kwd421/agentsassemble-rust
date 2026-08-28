use agentsassemble_domain::validate_room_id;
use tauri::AppHandle;

use super::{
    HttpTicketGrant, LocalRuntime, ManagerRoomAuthority,
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
    pub(crate) fn issue_human_invite_create_ticket(
        &self,
        app: &AppHandle,
        authority: ManagerRoomAuthority,
    ) -> Result<HttpTicketGrant, String> {
        self.issue_human_invite_ticket(app, authority, InviteTicketKind::Create)
    }

    /// Issues an invite-revoke-only HTTP ticket for one validated room manager.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid room, rejected manager authority, or broken owned runtime.
    pub(crate) fn issue_human_invite_revoke_ticket(
        &self,
        app: &AppHandle,
        authority: ManagerRoomAuthority,
    ) -> Result<HttpTicketGrant, String> {
        self.issue_human_invite_ticket(app, authority, InviteTicketKind::Revoke)
    }

    fn issue_human_invite_ticket(
        &self,
        app: &AppHandle,
        authority: ManagerRoomAuthority,
        kind: InviteTicketKind,
    ) -> Result<HttpTicketGrant, String> {
        let authority = validate_manager_room_authority(authority)?;
        let mut process = self
            .process
            .lock()
            .map_err(|_| "local runtime state lock is poisoned".to_owned())?;
        let runtime = ensure_runtime(&mut process, app)?;
        let result = match kind {
            InviteTicketKind::Create => request_human_invite_create_ticket(runtime, &authority),
            InviteTicketKind::Revoke => request_human_invite_revoke_ticket(runtime, &authority),
        };
        handle_ticket_result(&mut process, result)
    }
}

pub(super) fn validate_manager_room_authority(
    authority: ManagerRoomAuthority,
) -> Result<ManagerRoomAuthority, String> {
    let canonical_room = validate_room_id(&authority.room_id)
        .map_err(|error| format!("invalid room id: {}", error.message))?;
    if canonical_room != authority.room_id {
        return Err("room id must be supplied in canonical form".to_owned());
    }
    require_canonical_uuid(&authority.server_id, "server id")?;
    require_canonical_uuid(&authority.authority_lineage_id, "authority lineage id")?;
    require_canonical_uuid(&authority.room_uid, "room uid")?;
    Ok(authority)
}

fn require_canonical_uuid(value: &str, label: &str) -> Result<(), String> {
    let parsed = uuid::Uuid::parse_str(value).map_err(|_| format!("invalid {label}"))?;
    if parsed.to_string() != value {
        return Err(format!("{label} must be supplied in canonical form"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{ManagerRoomAuthority, require_canonical_uuid, validate_manager_room_authority};

    #[test]
    fn manager_authority_uuid_must_be_canonical() {
        let canonical = "10000000-0000-4000-8000-0000000000ab";
        assert_eq!(require_canonical_uuid(canonical, "server id"), Ok(()));
        assert!(require_canonical_uuid(&canonical.to_uppercase(), "server id").is_err());
        assert!(require_canonical_uuid("not-a-uuid", "server id").is_err());
    }

    #[test]
    fn manager_authority_room_id_must_be_unchanged() {
        assert!(
            validate_manager_room_authority(ManagerRoomAuthority {
                server_id: "10000000-0000-4000-8000-000000000001".to_owned(),
                authority_lineage_id: "20000000-0000-4000-8000-000000000002".to_owned(),
                room_id: " general".to_owned(),
                room_uid: "30000000-0000-4000-8000-000000000003".to_owned(),
            })
            .is_err()
        );
    }

    #[test]
    fn manager_authority_rejects_unknown_fields() {
        assert!(
            serde_json::from_value::<ManagerRoomAuthority>(json!({
                "server_id": "10000000-0000-4000-8000-000000000001",
                "authority_lineage_id": "20000000-0000-4000-8000-000000000002",
                "room_id": "general",
                "room_uid": "30000000-0000-4000-8000-000000000003",
                "extra": true
            }))
            .is_err()
        );
    }
}
