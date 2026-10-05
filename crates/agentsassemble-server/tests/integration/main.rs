// Keep suite names as module filters while linking the server only once.
// Run one suite with: cargo test -p agentsassemble-server --test integration runtime_boundary::

#[path = "../support"]
mod support {
    pub mod attendee;
    pub mod human_invite;
    pub mod human_profile_target;
    pub mod local_socket;
    pub mod lossy_http;
    #[cfg(unix)]
    pub mod native_interrupt_fixture;
    pub mod provider_fixture;
    #[cfg(unix)]
    pub mod room_portal_fixture;
    pub mod room_socket_peer;
}

#[path = "../agent_session_boundary.rs"]
mod agent_session_boundary;
#[path = "../attendee_boundary.rs"]
mod attendee_boundary;
#[path = "../attendee_cli.rs"]
mod attendee_cli;
#[path = "../attendee_client.rs"]
mod attendee_client;
#[path = "../attendee_client_execution.rs"]
mod attendee_client_execution;
#[path = "../attendee_client_interrupt.rs"]
mod attendee_client_interrupt;
#[path = "../attendee_client_runtime.rs"]
mod attendee_client_runtime;
#[path = "../attendee_socket.rs"]
mod attendee_socket;
#[path = "../central_login_boundary.rs"]
mod central_login_boundary;
#[path = "../central_login_only.rs"]
mod central_login_only;
#[path = "../central_owner_boundary.rs"]
mod central_owner_boundary;
#[path = "../channel_history_boundary.rs"]
mod channel_history_boundary;
#[path = "../channel_http_incarnation_boundary.rs"]
mod channel_http_incarnation_boundary;
#[path = "../connector_boundary.rs"]
mod connector_boundary;
#[path = "../control_pipe.rs"]
mod control_pipe;
#[path = "../guest_identity_recovery_boundary.rs"]
mod guest_identity_recovery_boundary;
#[path = "../human_invite_boundary.rs"]
mod human_invite_boundary;
#[path = "../human_invite_manager_boundary.rs"]
mod human_invite_manager_boundary;
#[path = "../human_session_socket_lifecycle.rs"]
mod human_session_socket_lifecycle;
#[path = "../ingress_boundary.rs"]
mod ingress_boundary;
#[path = "../local_attendee.rs"]
mod local_attendee;
#[path = "../local_attendee_web.rs"]
mod local_attendee_web;
#[path = "../message_attachments_boundary.rs"]
mod message_attachments_boundary;
#[path = "../message_mutations_boundary.rs"]
mod message_mutations_boundary;
#[path = "../message_pins_boundary.rs"]
mod message_pins_boundary;
#[path = "../message_search_boundary.rs"]
mod message_search_boundary;
#[path = "../operator_pairing_boundary.rs"]
mod operator_pairing_boundary;
#[path = "../participant_leave_boundary.rs"]
mod participant_leave_boundary;
#[path = "../participant_removal_boundary.rs"]
mod participant_removal_boundary;
#[path = "../participant_role_boundary.rs"]
mod participant_role_boundary;
#[path = "../persona_library_boundary.rs"]
mod persona_library_boundary;
#[path = "../persona_snapshot_capacity.rs"]
mod persona_snapshot_capacity;
#[path = "../profile_boundary.rs"]
mod profile_boundary;
#[path = "../provider_credentials_boundary.rs"]
mod provider_credentials_boundary;
#[path = "../provider_operations_boundary.rs"]
mod provider_operations_boundary;
#[path = "../provider_request_broker.rs"]
mod provider_request_broker;
#[path = "../room_appearance_session_boundary.rs"]
mod room_appearance_session_boundary;
#[path = "../room_directory_boundary.rs"]
mod room_directory_boundary;
#[path = "../room_history_boundary.rs"]
mod room_history_boundary;
#[path = "../room_lifecycle_boundary.rs"]
mod room_lifecycle_boundary;
#[path = "../room_preferences_boundary.rs"]
mod room_preferences_boundary;
#[path = "../room_settings_boundary.rs"]
mod room_settings_boundary;
#[path = "../room_snapshot_participants.rs"]
mod room_snapshot_participants;
#[path = "../room_vote_boundary.rs"]
mod room_vote_boundary;
#[path = "../runtime_boundary.rs"]
mod runtime_boundary;
#[path = "../runtime_image.rs"]
mod runtime_image;
#[path = "../runtime_restart_socket.rs"]
mod runtime_restart_socket;
#[path = "../side_chat_boundary.rs"]
mod side_chat_boundary;
#[path = "../side_chat_live_boundary.rs"]
mod side_chat_live_boundary;

#[path = "../member_invite_boundary.rs"]
mod member_invite_boundary;
