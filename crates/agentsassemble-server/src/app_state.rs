use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Arc,
};

use agentsassemble_persistence::{PersistenceError, SqliteStore};
use agentsassemble_protocol::ServerProductSurface;
use agentsassemble_provider::{ProviderAdapter, ProviderCatalogService, ProviderCredentialStore};
use thiserror::Error;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use crate::{
    CentralHostIdentity, HostIdentityError, HumanInviteCredentialAuthority, RoomRuntime,
    TicketStore,
    connection_admission::ConnectionAdmission,
    public_ingress::{ManualPublicIngressError, PublicIngress, PublicIngressControlError},
    socket_admission::SocketAdmission,
    stable_entry::{StableEntryActivationError, StableEntryConfig},
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
    pub google_accounts: crate::GoogleAccountService,
    pub rooms: RoomRuntime,
    pub tickets: TicketStore,
    pub provider_catalog: ProviderCatalogService,
    pub runtime_restart: crate::runtime_restart::RuntimeRestartControl,
    pub(crate) local_resources: crate::local_resources::LocalResources,
    pub provider_usage: agentsassemble_provider::ProviderUsageService,
    pub provider_update: agentsassemble_provider::ProviderUpdateService,
    pub provider_login: agentsassemble_provider::ProviderLoginService,
    pub local_attendees: crate::LocalAttendeeService,
    pub provider_adapter: ProviderAdapter,
    pub(crate) provider_credentials: ProviderCredentialStore,
    pub human_invite_credentials: HumanInviteCredentialAuthority,
    pub(crate) central_host_identity: CentralHostIdentity,
    pub(crate) central_login: crate::central_login::CentralLoginBroker,
    pub shutdown: CancellationToken,
    pub connections: TaskTracker,
    pub(crate) connection_admission: ConnectionAdmission,
    pub(crate) socket_admission: SocketAdmission,
    pub(crate) recovery_attempts: Arc<crate::guest_recovery_attempts::GuestRecoveryAttempts>,
    pub(crate) public_ingress: PublicIngress,
    pub server_product_surface: Arc<ServerProductSurface>,
    pub(crate) frontend: Option<Arc<crate::frontend_release::FrontendRelease>>,
    pub(crate) runtime_state_root: Option<PathBuf>,
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
        provider_catalog: ProviderCatalogService,
    ) -> Result<Self, AppStateBuildError> {
        let provider_credentials = ProviderCredentialStore::production();
        let provider_adapter = ProviderAdapter::with_credentials(provider_credentials.clone());
        Self::local_with_provider_dependencies(
            store,
            tickets,
            provider_catalog,
            provider_adapter,
            provider_credentials,
        )
        .await
    }

    /// Builds the production local runtime with provider state beside the database.
    ///
    /// # Errors
    ///
    /// Rejects missing or malformed persistent host identity state.
    pub async fn local_with_provider_state_root(
        store: SqliteStore,
        tickets: TicketStore,
        provider_catalog: ProviderCatalogService,
        state_root: &Path,
        provider_credentials: ProviderCredentialStore,
    ) -> Result<Self, AppStateBuildError> {
        let provider_adapter = ProviderAdapter::with_credentials_and_state_root(
            provider_credentials.clone(),
            state_root,
        );
        let mut state = Self::local_with_provider_dependencies(
            store,
            tickets,
            provider_catalog,
            provider_adapter,
            provider_credentials,
        )
        .await?;
        state.runtime_state_root = Some(state_root.to_owned());
        Ok(state)
    }

    /// Builds a local runtime with the database-bound central host identity.
    ///
    /// # Errors
    ///
    /// Rejects missing or malformed persistent host identity state.
    pub async fn local_with_provider_adapter(
        store: SqliteStore,
        tickets: TicketStore,
        provider_catalog: ProviderCatalogService,
        provider_adapter: ProviderAdapter,
    ) -> Result<Self, AppStateBuildError> {
        Self::local_with_provider_dependencies(
            store,
            tickets,
            provider_catalog,
            provider_adapter,
            ProviderCredentialStore::production(),
        )
        .await
    }

    async fn local_with_provider_dependencies(
        store: SqliteStore,
        tickets: TicketStore,
        provider_catalog: ProviderCatalogService,
        provider_adapter: ProviderAdapter,
        provider_credentials: ProviderCredentialStore,
    ) -> Result<Self, AppStateBuildError> {
        let persistent_host_identity = store.host_identity().await?;
        let human_invite_credentials =
            HumanInviteCredentialAuthority::from_persistent(&persistent_host_identity);
        let central_host_identity =
            CentralHostIdentity::from_persistent(&persistent_host_identity)?;
        let shutdown = CancellationToken::new();
        let local_attendees =
            crate::LocalAttendeeService::new(store.clone(), shutdown.child_token());
        Ok(Self {
            rooms: RoomRuntime::with_provider_adapter(
                store.clone(),
                provider_catalog.clone(),
                provider_adapter.clone(),
            ),
            store,
            google_accounts: crate::GoogleAccountService::default(),
            tickets,
            provider_catalog: provider_catalog.clone(),
            runtime_restart: crate::runtime_restart::RuntimeRestartControl::default(),
            local_resources: crate::local_resources::LocalResources::default(),
            provider_update: agentsassemble_provider::ProviderUpdateService::new(
                shutdown.child_token(),
                provider_catalog.clone(),
            ),
            provider_login: agentsassemble_provider::ProviderLoginService::new(
                shutdown.child_token(),
                provider_catalog,
            ),
            provider_adapter,
            local_attendees,
            provider_usage: agentsassemble_provider::ProviderUsageService::new(
                provider_credentials.clone(),
                shutdown.child_token(),
            ),
            provider_credentials,
            human_invite_credentials,
            central_host_identity,
            central_login: crate::central_login::CentralLoginBroker::default(),
            shutdown,
            connections: TaskTracker::new(),
            connection_admission: ConnectionAdmission::new(),
            socket_admission: SocketAdmission::new(),
            recovery_attempts: Arc::new(
                crate::guest_recovery_attempts::GuestRecoveryAttempts::default(),
            ),
            public_ingress: PublicIngress::disabled(),
            server_product_surface: Arc::new(crate::product_surface::server_product_surface(
                false, false,
            )),
            frontend: None,
            runtime_state_root: None,
            central_registration_enabled: false,
        })
    }

    #[must_use]
    pub fn with_frontend(mut self, frontend: crate::frontend_release::FrontendRelease) -> Self {
        self.frontend = Some(Arc::new(frontend));
        self.refresh_product_surface();
        self
    }

    #[must_use]
    pub fn with_central_registration(mut self) -> Self {
        self.central_registration_enabled = true;
        self.refresh_product_surface();
        self
    }

    /// Enables one immutable, startup-configured HTTPS reverse-proxy boundary.
    ///
    /// # Errors
    ///
    /// Rejects a non-public origin or a weak/unrepresentable proxy credential.
    pub fn with_manual_public_ingress(
        mut self,
        listener: SocketAddr,
        origin: &str,
        proxy_secret: &str,
    ) -> Result<Self, ManualPublicIngressError> {
        self.public_ingress = PublicIngress::configured_manual(listener, origin, proxy_secret)?;
        Ok(self)
    }

    /// Enables the managed ingress and claims configured stable publication ownership.
    ///
    /// # Errors
    ///
    /// Fails when configured stable-entry ownership cannot be claimed safely.
    pub async fn with_managed_public_ingress(
        mut self,
        listener: SocketAddr,
        stable_entry: Option<StableEntryConfig>,
        state_root: &Path,
    ) -> Result<Self, StableEntryActivationError> {
        self.public_ingress = PublicIngress::managed(listener, stable_entry, state_root).await?;
        Ok(self)
    }

    /// Closes every managed public-ingress effect owned by this server state.
    ///
    /// # Errors
    ///
    /// Fails when the exact managed child or stable-entry cleanup cannot be confirmed.
    pub async fn shutdown_public_ingress(&self) -> Result<(), PublicIngressControlError> {
        self.public_ingress.shutdown().await
    }

    pub(crate) fn public_ingress(&self) -> PublicIngress {
        self.public_ingress.clone()
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
            self.frontend.is_some(),
            self.central_registration_enabled,
        ));
    }
}
