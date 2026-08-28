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
    pub fn issue_human_invite_create_ticket(
        &self,
        app: &AppHandle,
        server_id: &str,
        authority_lineage_id: &str,
        requested_room_id: &str,
        room_uid: &str,
    ) -> Result<HttpTicketGrant, String> {
        self.issue_human_invite_ticket(
            app,
            server_id,
            authority_lineage_id,
            requested_room_id,
            room_uid,
            InviteTicketKind::Create,
        )
    }

    /// Issues an invite-revoke-only HTTP ticket for one validated room manager.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid room, rejected manager authority, or broken owned runtime.
    pub fn issue_human_invite_revoke_ticket(
        &self,
        app: &AppHandle,
        server_id: &str,
        authority_lineage_id: &str,
        requested_room_id: &str,
        room_uid: &str,
    ) -> Result<HttpTicketGrant, String> {
        self.issue_human_invite_ticket(
            app,
            server_id,
            authority_lineage_id,
            requested_room_id,
            room_uid,
            InviteTicketKind::Revoke,
        )
    }

    fn issue_human_invite_ticket(
        &self,
        app: &AppHandle,
        server_id: &str,
        authority_lineage_id: &str,
        requested_room_id: &str,
        room_uid: &str,
        kind: InviteTicketKind,
    ) -> Result<HttpTicketGrant, String> {
        let authority = validate_manager_room_authority(
            server_id,
            authority_lineage_id,
            requested_room_id,
            room_uid,
        )?;
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

fn validate_manager_room_authority(
    server_id: &str,
    lineage_id: &str,
    requested_room: &str,
    stable_room_uid: &str,
) -> Result<ManagerRoomAuthority, String> {
    let canonical_room = validate_room_id(requested_room)
        .map_err(|error| format!("invalid room id: {}", error.message))?;
    if canonical_room != requested_room {
        return Err("room id must be supplied in canonical form".to_owned());
    }
    Ok(ManagerRoomAuthority {
        server_id: canonical_uuid(server_id, "server id")?,
        authority_lineage_id: canonical_uuid(lineage_id, "authority lineage id")?,
        room_id: canonical_room,
        room_uid: canonical_uuid(stable_room_uid, "room uid")?,
    })
}

fn canonical_uuid(value: &str, label: &str) -> Result<String, String> {
    let parsed = uuid::Uuid::parse_str(value).map_err(|_| format!("invalid {label}"))?;
    if parsed.to_string() != value {
        return Err(format!("{label} must be supplied in canonical form"));
    }
    Ok(value.to_owned())
}

#[cfg(test)]
mod tests {
    use super::{canonical_uuid, validate_manager_room_authority};

    #[test]
    fn manager_authority_uuid_must_be_canonical() {
        let canonical = "10000000-0000-4000-8000-0000000000ab";
        assert_eq!(
            canonical_uuid(canonical, "server id").as_deref(),
            Ok(canonical)
        );
        assert!(canonical_uuid(&canonical.to_uppercase(), "server id").is_err());
        assert!(canonical_uuid("not-a-uuid", "server id").is_err());
    }

    #[test]
    fn manager_authority_room_id_must_be_unchanged() {
        assert!(
            validate_manager_room_authority(
                "10000000-0000-4000-8000-000000000001",
                "20000000-0000-4000-8000-000000000002",
                " general",
                "30000000-0000-4000-8000-000000000003",
            )
            .is_err()
        );
    }
}
