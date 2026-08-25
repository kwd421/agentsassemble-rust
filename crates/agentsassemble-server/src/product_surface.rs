use agentsassemble_protocol::{HttpRouteSurface, ServerProductSurface};

const STATIC_ROUTES: &[(agentsassemble_protocol::HttpMethod, &str)] = &[
    (agentsassemble_protocol::HttpMethod::Get, "/"),
    (agentsassemble_protocol::HttpMethod::Get, "/app/{*path}"),
];

pub(crate) struct RegisteredHttpRoute {
    pub(crate) method: agentsassemble_protocol::HttpMethod,
    pub(crate) path: &'static str,
}

pub(crate) fn server_product_surface(
    frontend_enabled: bool,
    central_registration_enabled: bool,
) -> ServerProductSurface {
    let mut routes = Vec::new();
    extend_registered(&mut routes, crate::web::HTTP_ROUTES);
    extend_registered(&mut routes, crate::room_directory_web::HTTP_ROUTES);
    extend_registered(&mut routes, crate::room_preferences_web::HTTP_ROUTES);
    extend_registered(&mut routes, crate::profile_web::HTTP_ROUTES);
    if central_registration_enabled {
        extend_registered(&mut routes, crate::central_registration_web::HTTP_ROUTES);
    }
    if frontend_enabled {
        routes.extend(
            STATIC_ROUTES
                .iter()
                .map(|(method, path)| HttpRouteSurface::new(*method, *path)),
        );
    }
    ServerProductSurface::from_http_routes(routes)
        .unwrap_or_else(|error| panic!("invalid server product-surface registry: {error}"))
}

fn extend_registered(target: &mut Vec<HttpRouteSurface>, descriptors: &[RegisteredHttpRoute]) {
    target.extend(
        descriptors
            .iter()
            .map(|descriptor| HttpRouteSurface::new(descriptor.method, descriptor.path)),
    );
}

#[cfg(test)]
mod tests {
    use super::server_product_surface;

    #[test]
    fn bundled_surface_adds_only_the_static_routes() {
        let server = server_product_surface(false, false);
        let bundled = server_product_surface(true, false);
        assert_eq!(bundled.http_routes.len(), server.http_routes.len() + 2);
        assert_ne!(bundled.digest, server.digest);
        assert!(
            bundled
                .http_routes
                .iter()
                .any(|route| route.path == "/app/{*path}")
        );
    }
}
