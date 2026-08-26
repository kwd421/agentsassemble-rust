use std::{path::PathBuf, sync::Arc};

use agentsassemble_persistence::{PersistenceError, SqliteStore};
use agentsassemble_protocol::ServerProductSurface;
use agentsassemble_provider::{ProviderAdapter, ProviderCatalogService};
use thiserror::Error;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use crate::{
    CentralHostIdentity, HostIdentityError, HostSecret, HumanInviteCredentialAuthority,
    HumanSessionBearerAuthority, RoomRuntime, TicketStore,
    connection_admission::ConnectionAdmission, raw_ingress::RawIngressGovernor,
};

#[derive(Debug, Error)]
pub enum AppStateBuildError {
    #[error("host identity persistence failed")]
    Persistence(#[from] PersistenceError),
    #[error("host identity projection failed")]
    HostIdentity(#[from] HostIdentityError),
}

#[derive(Clone)]
pub struct AppState {
    pub store: SqliteStore,
    pub rooms: RoomRuntime,
    pub tickets: TicketStore,
    pub host_token: HostSecret,
    pub provider_catalog: ProviderCatalogService,
    pub provider_adapter: ProviderAdapter,
    pub human_invite_credentials: HumanInviteCredentialAuthority,
    pub human_session_bearers: HumanSessionBearerAuthority,
    pub(crate) central_host_identity: CentralHostIdentity,
    pub shutdown: CancellationToken,
    pub connections: TaskTracker,
    pub(crate) connection_admission: ConnectionAdmission,
    pub(crate) raw_ingress: RawIngressGovernor,
    pub server_product_surface: Arc<ServerProductSurface>,
    pub frontend_root: Option<PathBuf>,
    pub(crate) central_registration_enabled: bool,
}

impl AppState {
    /// Builds a local runtime with the default provider adapter.
    ///
    /// # Errors
    ///
    /// Rejects missing or malformed persistent host identity state.
    pub async fn local(
        store: SqliteStore,
        tickets: TicketStore,
        host_token: HostSecret,
        provider_catalog: ProviderCatalogService,
    ) -> Result<Self, AppStateBuildError> {
        Self::local_with_provider_adapter(
            store,
            tickets,
            host_token,
            provider_catalog,
            ProviderAdapter::new(),
        )
        .await
    }

    /// Builds a local runtime with the database-bound central host identity.
    ///
    /// # Errors
    ///
    /// Rejects missing or malformed persistent host identity state.
    pub async fn local_with_provider_adapter(
        store: SqliteStore,
        tickets: TicketStore,
        host_token: HostSecret,
        provider_catalog: ProviderCatalogService,
        provider_adapter: ProviderAdapter,
    ) -> Result<Self, AppStateBuildError> {
        let persistent_host_identity = store.host_identity().await?;
        let human_invite_credentials =
            HumanInviteCredentialAuthority::from_persistent(&persistent_host_identity);
        let human_session_bearers =
            HumanSessionBearerAuthority::from_persistent(&persistent_host_identity);
        let central_host_identity =
            CentralHostIdentity::from_persistent(&persistent_host_identity)?;
        Ok(Self {
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
            human_invite_credentials,
            human_session_bearers,
            central_host_identity,
            shutdown: CancellationToken::new(),
            connections: TaskTracker::new(),
            connection_admission: ConnectionAdmission::new(),
            raw_ingress: RawIngressGovernor::new(),
            server_product_surface: Arc::new(crate::product_surface::server_product_surface(
                false, false,
            )),
            frontend_root: None,
            central_registration_enabled: false,
        })
    }

    #[must_use]
    pub fn with_frontend(mut self, frontend_root: PathBuf) -> Self {
        self.frontend_root = Some(frontend_root);
        self.refresh_product_surface();
        self
    }

    #[must_use]
    pub fn with_central_registration(mut self) -> Self {
        self.central_registration_enabled = true;
        self.refresh_product_surface();
        self
    }

    #[must_use]
    pub fn central_registration_binding(&self) -> (&str, &str, &str) {
        (
            self.central_host_identity.server_id(),
            self.central_host_identity.public_key_x(),
            self.central_host_identity.fingerprint(),
        )
    }

    fn refresh_product_surface(&mut self) {
        self.server_product_surface = Arc::new(crate::product_surface::server_product_surface(
            self.frontend_root.is_some(),
            self.central_registration_enabled,
        ));
    }
}
