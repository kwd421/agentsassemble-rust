use axum::{
    http::{HeaderValue, StatusCode, header},
    response::Response,
};

// One policy owner; only the documented GIS resource sources vary with configuration.
macro_rules! content_policy {
    ($script:literal, $style:literal, $connect:literal, $frame:literal) => {
        concat!(
            "default-src 'self'; script-src 'self'", $script,
            "; style-src 'self'", $style,
            "; connect-src 'self' http://127.0.0.1:* ws://127.0.0.1:*", $connect,
            "; frame-src 'self'", $frame,
            "; img-src 'self' data: blob: http://127.0.0.1:*; object-src 'none'; base-uri 'none'; frame-ancestors 'none'"
        )
    };
}

pub(crate) fn content_security_policy(google_enabled: bool) -> HeaderValue {
    HeaderValue::from_static(if google_enabled {
        content_policy!(
            " https://accounts.google.com/gsi/client",
            " https://accounts.google.com/gsi/style",
            " https://accounts.google.com/gsi/",
            " https://accounts.google.com/gsi/"
        )
    } else {
        content_policy!("", "", "", "")
    })
}

pub(crate) async fn apply(mut response: Response) -> Response {
    let is_upgrade = response.status() == StatusCode::SWITCHING_PROTOCOLS;
    let headers = response.headers_mut();
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::HeaderName::from_static("x-frame-options"),
        HeaderValue::from_static("DENY"),
    );
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    if !is_upgrade {
        headers.insert(header::CONNECTION, HeaderValue::from_static("close"));
    }
    response
}
