//! Direct local workspace transport, protected against rebinding and cross-origin requests.
use crate::{
    http_authorization, http_response,
    operation_error::Error,
    runtime_mailbox::{self, Envelope},
    runtime_protocol::Request,
};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, State, rejection::JsonRejection},
    http::{HeaderMap, StatusCode},
    response::Response,
    routing::post,
};
use tokio::sync::mpsc;
#[derive(Clone)]
struct ApiState {
    port: u16,
    demo: bool,
    commands: mpsc::Sender<Envelope>,
}
pub fn router(port: u16, demo: bool, commands: mpsc::Sender<Envelope>) -> Router {
    Router::new()
        .route("/v1/browser/info", post(info))
        .route("/v1/browser/command", post(command))
        .layer(DefaultBodyLimit::max(131072))
        .with_state(ApiState {
            port,
            demo,
            commands,
        })
}
async fn info(State(s): State<ApiState>, headers: HeaderMap) -> Response {
    if !http_authorization::browser_request(&headers, s.port) {
        return http_response::error(StatusCode::UNAUTHORIZED, Error::Unauthorized);
    }
    http_response::result(Ok(serde_json::json!({"demo":s.demo})))
}
async fn command(
    State(s): State<ApiState>,
    headers: HeaderMap,
    body: std::result::Result<Json<Request>, JsonRejection>,
) -> Response {
    if !http_authorization::browser_request(&headers, s.port) {
        return http_response::error(StatusCode::UNAUTHORIZED, Error::Unauthorized);
    }
    let Ok(Json(request)) = body else {
        return http_response::error(StatusCode::BAD_REQUEST, Error::InvalidRequest);
    };
    // UI availability policy; trusted local operators have full account authority.
    if matches!(request, Request::Shutdown) {
        return http_response::error(StatusCode::FORBIDDEN, Error::Unauthorized);
    }
    http_response::result(runtime_mailbox::invoke(&s.commands, request).await)
}
