macro_rules! http_method {
    (get) => {
        agentsassemble_protocol::HttpMethod::Get
    };
    (post) => {
        agentsassemble_protocol::HttpMethod::Post
    };
    (delete) => {
        agentsassemble_protocol::HttpMethod::Delete
    };
}

macro_rules! secure_access {
    (secure_remote) => {
        crate::product_surface::SecureAccess::Product
    };
    (secure_owner) => {
        crate::product_surface::SecureAccess::Admission("owner")
    };
    (secure_member) => {
        crate::product_surface::SecureAccess::Admission("member_admission")
    };
    (secure_connect) => {
        crate::product_surface::SecureAccess::Admission("member_connect")
    };
    ($other:ident) => {
        crate::product_surface::SecureAccess::Excluded
    };
}

macro_rules! route_exposure {
    (secure_remote) => {
        crate::product_surface::RouteExposure::SameOriginPublic
    };
    (secure_owner) => {
        crate::product_surface::RouteExposure::SameOriginPublic
    };
    (secure_member) => {
        crate::product_surface::RouteExposure::SameOriginPublic
    };
    (secure_connect) => {
        crate::product_surface::RouteExposure::SameOriginPublic
    };
    (private) => {
        crate::product_surface::RouteExposure::Private
    };
    (same_origin_public) => {
        crate::product_surface::RouteExposure::SameOriginPublic
    };
    (identity_probe_public) => {
        crate::product_surface::RouteExposure::IdentityProbePublic
    };
}

macro_rules! registered_routes {
    ($visibility:vis fn $function:ident<$state:ty>() {
        $($exposure:ident $path:literal => $first_method:ident($first_handler:expr)
            $(.$more_method:ident($more_handler:expr))*),+ $(,)?
    }) => {
        pub(crate) const HTTP_ROUTES: &[crate::product_surface::RegisteredHttpRoute] = &[
            $(
                crate::product_surface::RegisteredHttpRoute {
                    method: http_method!($first_method),
                    path: $path,
                    exposure: route_exposure!($exposure),
                    secure: secure_access!($exposure),
                },
                $(
                    crate::product_surface::RegisteredHttpRoute {
                        method: http_method!($more_method),
                        path: $path,
                        exposure: route_exposure!($exposure),
                    secure: secure_access!($exposure),
                    },
                )*
            )+
        ];

        $visibility fn $function() -> axum::Router<$state> {
            axum::Router::new()
                $(.route(
                    $path,
                    axum::routing::$first_method($first_handler)
                        $(.$more_method($more_handler))*
                ))+
        }
    };
}

mod agent_create_runtime;
mod app_state;
mod attendee;
mod central;
mod frontend_assets;
mod frontend_document;
pub mod frontend_release;
mod google_accounts;
mod google_token_verifier;
mod owner_devices_web;
mod owner_session_lifetime;
#[cfg(unix)]
pub mod runtime_control_socket;
pub mod runtime_image;
pub mod runtime_restart;
#[cfg(unix)]
pub mod runtime_restart_ipc;
mod runtime_restart_web;
pub mod runtime_version;
pub use central::login::{CentralLoginService, central_login_control};
pub use google_accounts::{GoogleAccountError, GoogleAccountService};
mod account_web;
mod local_attendee;
mod local_attendee_web;
pub use attendee::client_shutdown::shutdown_attendee;
pub use attendee::client_tools::AttendeeToolCall;
pub use attendee::tool_wire::AttendeeToolReadResponse;
pub use local_attendee::{LocalAttendeeError, LocalAttendeeService};
mod connection_admission;
pub mod connector;
mod event_publication;
mod friends_web;
mod guest_identity_recovery_web;
mod guest_recovery_attempts;
mod http_admission;
mod http_api;
mod http_transport;
mod human_admission_runtime;
mod human_browser_credential;
mod human_invite_credentials;
mod human_invite_manager_web;
mod human_invite_preflight;
mod human_invite_web;
mod human_session_bearer;
mod human_session_exchange_web;
mod human_session_http_authority;
mod ingress_trust;
mod lifecycle_command_tracker;
mod local_resources;
mod message_pins_web;
mod message_search_web;
mod operational_web;
mod operator_pairing_web;
mod owned_command;
mod persona_web;
mod principal_mutation_admission;
mod product_surface;
mod profile_web;
mod provider_attachment_runtime;
mod provider_credentials_web;
mod provider_operations_web;
mod provider_recovery_tracker;
mod provider_room_tool_runtime;
mod provider_turn;
mod provider_turn_interrupt_runtime;
mod provider_turn_reconciliation_runtime;
mod provider_write_budget;
mod public_ingress;
mod public_ingress_process;
mod public_ingress_runtime;
mod public_ingress_web;
pub mod release_health;
mod room_agent_lifecycle_runtime;
mod room_channel;
mod room_client_transport;
mod secure_channel;
mod secure_client;
mod secure_http;
mod secure_queue;
mod secure_socket;
pub use attendee::client::{AttendeeClientError, AttendeeJoined, RoomAttendeeClient};
pub use attendee::client_run::run_attendee_session;
pub use attendee::client_runtime::{AttendeeExecution, AttendeeInterrupt, AttendeeRuntime};
pub use attendee::client_socket::AttendeeSocket;
pub use attendee::wire::{AttendeeSocketFailure, AttendeeSocketFrame, AttendeeSocketRequest};
mod room_command_admission;
mod room_command_dispatch;
mod room_command_execution;
mod room_command_result;
mod room_directory_stream;
mod room_directory_web;
mod room_history_socket;
mod room_preferences_web;
mod room_random_runtime;
mod room_recovery_runtime;
mod room_runtime;
mod session_revocation;
pub use session_revocation::SessionRevocation;
mod room_runtime_cleanup;
mod room_session_http_authority;
mod room_shutdown;
mod room_socket;
mod room_socket_direct;
#[cfg(test)]
mod room_socket_owner_tests;
mod room_socket_session;
mod room_vote_socket;
mod runtime_reconciliation;
mod runtime_reconciliation_cleanup;
mod security_headers;
mod server_identity_web;
mod side_chat_socket;
mod side_chat_web;
mod socket_admission;
mod stable_entry;
mod ticket;
mod ticket_issuer;
#[cfg(test)]
mod ticket_tests;
mod web;

pub use app_state::{AppState, AppStateBuildError};
pub use central::directory::CentralDirectoryError;
pub use central::host_identity::{CentralHostIdentity, HostIdentityError, host_device_info};
pub use human_invite_credentials::{
    HumanInviteCredentialAuthority, HumanInviteCredentialDraft, HumanInviteCredentialError,
    IssuedHumanInviteCredentials, VerifiedHumanInviteClaims, VerifiedHumanInviteCredential,
};
pub use human_invite_preflight::{HumanInvitePreflightError, preflight_human_invite};
pub use ingress_trust::local_bind_is_supported;
pub use public_ingress::{ManualPublicIngressError, PublicIngressControlError};
pub use room_runtime::{LiveProviderRequest, ResolvedProviderRequest, RoomRuntime};
pub use room_shutdown::RoomShutdownError;
pub use runtime_reconciliation::{RuntimeReconciliationSummary, reconcile_runtime_ownership};
pub use stable_entry::{StableEntryActivationError, StableEntryConfig, StableEntryConfigError};
pub use ticket::{
    ConsumedCentralRegistrationTicket, ConsumedProfileTicket, ConsumedRoomHttpTicket,
    ConsumedRoomSessionSocketTicket, ConsumedServerOperatorTicket,
    ConsumedSettingsDirectoryReadTicket, ConsumedTicket, IssuedTicket, RoomHttpPurpose,
    TicketError, TicketStore,
};
pub use ticket_issuer::{
    ManagerRoomAuthorityRequest, TicketIssueError, issue_agent_avatar_upload_ticket,
    issue_appearance_bound_read_ticket, issue_appearance_pending_read_ticket,
    issue_appearance_upload_ticket, issue_attendee_invite_create_ticket,
    issue_central_registration_ticket, issue_connector_invite_create_ticket,
    issue_human_invite_create_ticket, issue_human_invite_revoke_ticket,
    issue_local_operator_http_ticket, issue_local_ticket, issue_message_attachment_read_ticket,
    issue_message_attachment_upload_ticket, issue_message_pins_read_ticket,
    issue_message_pins_write_ticket, issue_message_search_read_ticket,
    issue_preferences_read_ticket, issue_preferences_write_ticket,
    issue_settings_directory_read_ticket, issue_side_chat_read_ticket,
};
pub use web::{ServeError, router, serve};

pub use room_runtime::{AttendeeOperation, AttendeeOperationResult};
