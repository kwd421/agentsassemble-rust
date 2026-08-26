use std::time::Duration;

use agentsassemble_domain::{AuthenticatedPrincipal, CapabilitySet, ClientKind, InviteScope};

use crate::{TicketError, TicketStore};

fn principal() -> AuthenticatedPrincipal {
    AuthenticatedPrincipal {
        principal_id: "operator".to_owned(),
        participant_id: "operator-local".to_owned(),
        display_name: "Host".to_owned(),
        room_id: "general".to_owned(),
        client_kind: ClientKind::Browser,
        invite_scope: InviteScope::ReadWrite,
        is_operator: true,
        capabilities: CapabilitySet::local_operator(ClientKind::Browser, InviteScope::ReadWrite),
    }
}

#[tokio::test]
async fn ticket_is_consumed_once() {
    let store = TicketStore::new(Duration::from_secs(30), 8);
    let ticket = store
        .issue(principal())
        .await
        .unwrap_or_else(|error| panic!("issue ticket: {error}"));
    assert!(store.consume(&ticket.ticket).await.is_ok());
    assert_eq!(
        store.consume(&ticket.ticket).await,
        Err(TicketError::Invalid)
    );
}

#[tokio::test]
async fn expired_ticket_fails_closed() {
    let store = TicketStore::new(Duration::ZERO, 8);
    let ticket = store
        .issue(principal())
        .await
        .unwrap_or_else(|error| panic!("issue ticket: {error}"));
    assert_eq!(
        store.consume(&ticket.ticket).await,
        Err(TicketError::Invalid)
    );
}

#[tokio::test]
async fn ticket_purposes_are_one_use_and_never_interchangeable() {
    let store = TicketStore::new(Duration::from_secs(30), 8);
    let operator = store
        .issue_server_operator("operator-local-user".to_owned())
        .await
        .unwrap_or_else(|error| panic!("issue operator ticket: {error}"));
    assert_eq!(
        store.consume(&operator.ticket).await,
        Err(TicketError::Invalid)
    );
    assert_eq!(
        store.consume_server_operator(&operator.ticket).await,
        Err(TicketError::Invalid)
    );

    let room = store
        .issue(principal())
        .await
        .unwrap_or_else(|error| panic!("issue room ticket: {error}"));
    assert_eq!(
        store.consume_server_operator(&room.ticket).await,
        Err(TicketError::Invalid)
    );
    assert_eq!(store.consume(&room.ticket).await, Err(TicketError::Invalid));
}

#[tokio::test]
async fn central_registration_ticket_is_not_generic_operator_or_profile_authority() {
    let store = TicketStore::new(Duration::from_secs(30), 8);
    let registration = store
        .issue_central_registration("operator-local-user".to_owned())
        .await
        .unwrap_or_else(|error| panic!("issue registration ticket: {error}"));
    assert_eq!(
        store.consume_server_operator(&registration.ticket).await,
        Err(TicketError::Invalid)
    );
    assert_eq!(
        store
            .consume_central_registration(&registration.ticket)
            .await,
        Err(TicketError::Invalid)
    );

    let profile_rejected = store
        .issue_central_registration("operator-local-user".to_owned())
        .await
        .unwrap_or_else(|error| panic!("issue profile-rejected ticket: {error}"));
    assert_eq!(
        store.consume_profile(&profile_rejected.ticket).await,
        Err(TicketError::Invalid)
    );
    assert_eq!(
        store
            .consume_central_registration(&profile_rejected.ticket)
            .await,
        Err(TicketError::Invalid)
    );
}

#[tokio::test]
async fn room_http_purposes_and_asset_bindings_are_consumed_on_mismatch() {
    let store = TicketStore::new(Duration::from_secs(30), 8);
    let preference = store
        .issue_preferences_read(
            "general".to_owned(),
            "operator-local-user".to_owned(),
            "operator-local".to_owned(),
        )
        .await
        .unwrap_or_else(|error| panic!("issue preference read: {error}"));
    assert_eq!(
        store.consume_preferences_write(&preference.ticket).await,
        Err(TicketError::Invalid)
    );
    assert_eq!(
        store.consume_preferences_read(&preference.ticket).await,
        Err(TicketError::Invalid)
    );

    let asset = store
        .issue_pending_preview_read(
            "general".to_owned(),
            "operator-local-user".to_owned(),
            "operator-local".to_owned(),
            "ra_00000000000000000000000000000000".to_owned(),
        )
        .await
        .unwrap_or_else(|error| panic!("issue pending preview read: {error}"));
    assert_eq!(
        store
            .consume_pending_preview_read(&asset.ticket, "ra_11111111111111111111111111111111",)
            .await,
        Err(TicketError::Invalid)
    );
    assert_eq!(
        store
            .consume_pending_preview_read(&asset.ticket, "ra_00000000000000000000000000000000",)
            .await,
        Err(TicketError::Invalid)
    );
}

#[tokio::test]
async fn settings_directory_ticket_never_crosses_room_or_profile_scopes() {
    let store = TicketStore::new(Duration::from_secs(30), 8);
    let directory = store
        .issue_settings_directory_read("operator-local-user".to_owned())
        .await
        .unwrap_or_else(|error| panic!("issue directory read: {error}"));
    assert_eq!(
        store.consume_profile(&directory.ticket).await,
        Err(TicketError::Invalid)
    );
    assert_eq!(
        store
            .consume_settings_directory_read(&directory.ticket)
            .await,
        Err(TicketError::Invalid)
    );
}
