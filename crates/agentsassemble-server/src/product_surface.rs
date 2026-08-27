use agentsassemble_protocol::{HttpRouteSurface, ServerProductSurface};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RouteExposure {
    Private,
    SameOriginPublic,
    IdentityProbePublic,
}

pub(crate) struct RegisteredHttpRoute {
    pub(crate) method: agentsassemble_protocol::HttpMethod,
    pub(crate) path: &'static str,
    pub(crate) exposure: RouteExposure,
}

pub(crate) fn registered_route_exposure(
    method: agentsassemble_protocol::HttpMethod,
    path: &str,
) -> Option<RouteExposure> {
    registered_routes(true)
        .find(|route| route.method == method && route.path == path)
        .map(|route| route.exposure)
}

pub(crate) fn registered_route_path(path: &str) -> bool {
    registered_routes(true).any(|route| route.path == path)
}

pub(crate) fn server_product_surface(
    frontend_enabled: bool,
    central_registration_enabled: bool,
) -> ServerProductSurface {
    let mut routes = Vec::new();
    routes.extend(
        registered_routes(central_registration_enabled)
            .map(|descriptor| HttpRouteSurface::new(descriptor.method, descriptor.path)),
    );
    if frontend_enabled {
        routes.extend(crate::web::static_frontend_surfaces());
    }
    ServerProductSurface::from_http_routes(routes)
        .unwrap_or_else(|error| panic!("invalid server product-surface registry: {error}"))
}

fn registered_routes(
    central_registration_enabled: bool,
) -> impl Iterator<Item = &'static RegisteredHttpRoute> {
    [
        crate::web::HTTP_ROUTES,
        crate::room_directory_web::HTTP_ROUTES,
        crate::room_preferences_web::HTTP_ROUTES,
        crate::profile_web::HTTP_ROUTES,
        crate::human_invite_web::HTTP_ROUTES,
        crate::human_session_exchange_web::HTTP_ROUTES,
        crate::server_identity_web::HTTP_ROUTES,
    ]
    .into_iter()
    .flatten()
    .chain(
        central_registration_enabled
            .then_some(crate::central_registration_web::HTTP_ROUTES)
            .into_iter()
            .flatten(),
    )
}

#[cfg(test)]
mod tests {
    use agentsassemble_protocol::HttpMethod;

    use super::{
        RouteExposure, registered_route_exposure, registered_route_path, server_product_surface,
    };

    #[test]
    fn bundled_surface_adds_only_the_static_routes() {
        let server = server_product_surface(false, false);
        let bundled = server_product_surface(true, false);
        assert_eq!(bundled.http_routes.len(), server.http_routes.len() + 11);
        assert_ne!(bundled.digest, server.digest);
        assert!(
            bundled
                .http_routes
                .iter()
                .any(|route| route.path == "/app/{*path}")
        );
        assert!(
            bundled
                .http_routes
                .iter()
                .any(|route| route.path == "/join")
        );
    }

    #[test]
    fn dynamic_exposure_is_owned_by_the_registered_route() {
        assert_eq!(
            registered_route_exposure(HttpMethod::Get, "/ws"),
            Some(RouteExposure::SameOriginPublic)
        );
        assert_eq!(
            registered_route_exposure(HttpMethod::Post, "/api/ws-ticket"),
            Some(RouteExposure::Private)
        );
        assert_eq!(
            registered_route_exposure(HttpMethod::Post, "/api/room-invite/join"),
            Some(RouteExposure::SameOriginPublic)
        );
        assert_eq!(
            registered_route_exposure(HttpMethod::Get, "/api/server-info"),
            Some(RouteExposure::IdentityProbePublic)
        );
        assert_eq!(
            registered_route_exposure(HttpMethod::Post, "/api/server-info/challenge"),
            Some(RouteExposure::IdentityProbePublic)
        );
        assert_eq!(
            registered_route_exposure(HttpMethod::Get, "/api/room-invite/join"),
            None
        );
        assert!(registered_route_path("/api/room-invite/join"));
        assert!(!registered_route_path("/api/not-registered"));
    }

    #[test]
    fn registered_dynamic_method_paths_are_unique() {
        let mut seen = Vec::new();
        for route in super::registered_routes(true) {
            assert!(
                !seen.contains(&(route.method, route.path)),
                "duplicate route descriptor for {:?} {}",
                route.method,
                route.path
            );
            seen.push((route.method, route.path));
        }
    }
}
