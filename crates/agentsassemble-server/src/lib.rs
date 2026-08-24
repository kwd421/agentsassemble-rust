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
mod event_publication;
mod host_ticket;
mod http_api;
mod http_transport;
mod ingress_budget;
mod principal_write_budget;
mod product_surface;
mod profile_web;
mod provider_turn;
mod room_agent_lifecycle_runtime;
mod room_command_result;
mod room_directory_web;
mod room_random_runtime;
mod room_runtime;
mod room_shutdown;
mod room_socket;
mod runtime_reconciliation;
mod security_headers;
mod server_proof;
mod ticket;
mod ticket_issuer;
mod web;

pub use app_state::AppState;
pub use host_ticket::{HostSecret, InvalidHostSecret};
pub use room_runtime::{RoomRuntime, RoomShutdownError};
pub use runtime_reconciliation::reconcile_runtime_ownership;
pub use ticket::{
    ConsumedProfileTicket, ConsumedServerOperatorTicket, ConsumedTicket, IssuedTicket, TicketError,
    TicketStore,
};
pub use ticket_issuer::{TicketIssueError, issue_local_operator_http_ticket, issue_local_ticket};
pub use web::{ServeError, router, serve};
