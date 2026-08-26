macro_rules! http_method {
    (get) => {
        agentsassemble_protocol::HttpMethod::Get
    };
    (post) => {
        agentsassemble_protocol::HttpMethod::Post
    };
}

macro_rules! registered_routes {
    ($visibility:vis fn $function:ident<$state:ty>() {
        $($path:literal => $first_method:ident($first_handler:expr)
            $(.$more_method:ident($more_handler:expr))*),+ $(,)?
    }) => {
        pub(crate) const HTTP_ROUTES: &[crate::product_surface::RegisteredHttpRoute] = &[
            $(
                crate::product_surface::RegisteredHttpRoute {
                    method: http_method!($first_method),
                    path: $path,
                },
                $(
                    crate::product_surface::RegisteredHttpRoute {
                        method: http_method!($more_method),
                        path: $path,
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
mod authenticated_channel;
mod central_host_identity;
mod central_registration_web;
mod connection_admission;
mod event_publication;
mod host_ticket;
mod http_api;
mod http_transport;
mod human_admission_runtime;
mod human_browser_credential;
mod human_invite_credentials;
mod human_invite_preflight;
mod human_invite_web;
mod human_session_bearer;
mod human_session_exchange_web;
mod lifecycle_command_tracker;
mod participant_mute_runtime;
mod principal_mutation_admission;
mod product_surface;
mod profile_web;
mod provider_recovery_tracker;
mod provider_turn;
mod provider_turn_reconciliation_runtime;
mod provider_write_budget;
mod raw_ingress;
mod room_agent_lifecycle_runtime;
mod room_command_admission;
mod room_command_dispatch;
mod room_command_execution;
mod room_command_result;
mod room_directory_web;
mod room_preferences_web;
mod room_random_runtime;
mod room_recovery_runtime;
mod room_runtime;
mod room_shutdown;
mod room_socket;
mod runtime_reconciliation;
mod runtime_reconciliation_cleanup;
mod security_headers;
mod server_proof;
mod ticket;
mod ticket_issuer;
#[cfg(test)]
mod ticket_tests;
mod web;

pub use app_state::{AppState, AppStateBuildError};
pub use central_host_identity::{CentralHostIdentity, HostIdentityError};
pub use host_ticket::{HostSecret, InvalidHostSecret};
pub use human_invite_credentials::{
    HumanInviteCredentialAuthority, HumanInviteCredentialDraft, HumanInviteCredentialError,
    IssuedHumanInviteCredentials, VerifiedHumanInviteClaims, VerifiedHumanInviteCredential,
};
pub use human_invite_preflight::{HumanInvitePreflightError, preflight_human_invite};
pub use room_runtime::RoomRuntime;
pub use room_shutdown::RoomShutdownError;
pub use runtime_reconciliation::{RuntimeReconciliationSummary, reconcile_runtime_ownership};
pub use ticket::{
    ConsumedCentralRegistrationTicket, ConsumedHumanSessionSocketTicket, ConsumedProfileTicket,
    ConsumedRoomHttpTicket, ConsumedServerOperatorTicket, ConsumedSettingsDirectoryReadTicket,
    ConsumedTicket, IssuedTicket, RoomHttpPurpose, TicketError, TicketStore,
};
pub use ticket_issuer::{
    TicketIssueError, issue_central_registration_ticket, issue_local_operator_http_ticket,
    issue_local_ticket, issue_preferences_read_ticket, issue_preferences_write_ticket,
    issue_settings_directory_read_ticket,
};
pub use web::{ServeError, router, serve};
