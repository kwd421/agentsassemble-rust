use std::{path::PathBuf, sync::Arc};

use agentsassemble_persistence::SqliteStore;
use agentsassemble_protocol::ServerProductSurface;
use agentsassemble_provider::{ProviderAdapter, ProviderCatalogService};
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use crate::{
    HostSecret, RoomRuntime, TicketStore, connection_admission::ConnectionAdmission,
    raw_ingress::RawIngressGovernor,
};

#[derive(Clone)]
pub struct AppState {
    pub store: SqliteStore,
    pub rooms: RoomRuntime,
    pub tickets: TicketStore,
    pub host_token: HostSecret,
    pub provider_catalog: ProviderCatalogService,
    pub provider_adapter: ProviderAdapter,
    pub shutdown: CancellationToken,
    pub connections: TaskTracker,
    pub(crate) connection_admission: ConnectionAdmission,
    pub(crate) raw_ingress: RawIngressGovernor,
    pub server_product_surface: Arc<ServerProductSurface>,
    pub frontend_root: Option<PathBuf>,
}

impl AppState {
    #[must_use]
    pub fn local(
        store: SqliteStore,
        tickets: TicketStore,
        host_token: HostSecret,
        provider_catalog: ProviderCatalogService,
    ) -> Self {
        Self::local_with_provider_adapter(
            store,
            tickets,
            host_token,
            provider_catalog,
            ProviderAdapter::new(),
        )
    }

    #[must_use]
    pub fn local_with_provider_adapter(
        store: SqliteStore,
        tickets: TicketStore,
        host_token: HostSecret,
        provider_catalog: ProviderCatalogService,
        provider_adapter: ProviderAdapter,
    ) -> Self {
        Self {
            rooms: RoomRuntime::with_provider_adapter(
                store.clone(),
                provider_catalog.clone(),
                provider_adapter.clone(),
            ),
            store,
            tickets,
            host_token,
            provider_catalog,
            provider_adapter,
            shutdown: CancellationToken::new(),
            connections: TaskTracker::new(),
            connection_admission: ConnectionAdmission::new(),
            raw_ingress: RawIngressGovernor::new(),
            server_product_surface: Arc::new(crate::product_surface::server_product_surface(false)),
            frontend_root: None,
        }
    }

    #[must_use]
    pub fn with_frontend(mut self, frontend_root: PathBuf) -> Self {
        self.server_product_surface =
            Arc::new(crate::product_surface::server_product_surface(true));
        self.frontend_root = Some(frontend_root);
        self
    }
}
