//! Host validation and browser response policy for the complete loopback server.
use crate::{http_authorization, http_response, operation_error::Error};
use axum::{
    extract::{Request, State},
    http::{HeaderName, HeaderValue, StatusCode},
    middleware::Next,
    response::Response,
};
pub async fn enforce(State(port): State<u16>, request: Request, next: Next) -> Response {
    let mut response = if http_authorization::host(request.headers(), port) {
        next.run(request).await
    } else {
        http_response::error(StatusCode::UNAUTHORIZED, Error::Unauthorized)
    };
    for (name, value) in [
        (
            "content-security-policy",
            "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' data:; font-src 'self'; connect-src 'self'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'",
        ),
        ("referrer-policy", "no-referrer"),
        ("x-content-type-options", "nosniff"),
        ("x-frame-options", "DENY"),
        ("cache-control", "no-store"),
        (
            "permissions-policy",
            "camera=(), microphone=(), geolocation=()",
        ),
    ] {
        response.headers_mut().insert(
            HeaderName::from_static(name),
            HeaderValue::from_static(value),
        );
    }
    response
}
