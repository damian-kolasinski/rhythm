//! Browser pairing and authenticated command transport; credentials remain outside URLs.
use crate::{
    browser_access::BrowserAccess,
    http_authorization, http_response,
    local_access::Access,
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
use serde::Deserialize;
use tokio::sync::mpsc;
#[derive(Clone)]
struct ApiState {
    access: Access,
    browser: BrowserAccess,
    commands: mpsc::Sender<Envelope>,
}
pub fn router(access: Access, browser: BrowserAccess, commands: mpsc::Sender<Envelope>) -> Router {
    Router::new()
        .route("/v1/browser-code", post(issue))
        .route("/v1/browser/session", post(pair))
        .route("/v1/browser/disconnect", post(disconnect))
        .route("/v1/browser/command", post(command))
        .layer(DefaultBodyLimit::max(131072))
        .with_state(ApiState {
            access,
            browser,
            commands,
        })
}
async fn issue(State(s): State<ApiState>, headers: HeaderMap) -> Response {
    if !http_authorization::cli_origin(&headers, s.access.port)
        || !headers
            .get("authorization")
            .is_some_and(|h| s.access.accepts(h.as_bytes()))
    {
        return http_response::error(StatusCode::UNAUTHORIZED, Error::Unauthorized);
    }
    http_response::result(
        s.browser
            .issue_code()
            .map(|code| serde_json::json!({"code":code,"demo":s.browser.demo})),
    )
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Pair {
    code: String,
}
async fn pair(
    State(s): State<ApiState>,
    headers: HeaderMap,
    body: std::result::Result<Json<Pair>, JsonRejection>,
) -> Response {
    if !http_authorization::browser_origin(&headers, s.access.port) {
        return http_response::error(StatusCode::UNAUTHORIZED, Error::Unauthorized);
    }
    let Ok(Json(body)) = body else {
        return http_response::error(StatusCode::BAD_REQUEST, Error::InvalidRequest);
    };
    http_response::result(
        s.browser
            .pair(&body.code)
            .map(|token| serde_json::json!({"token":token,"demo":s.browser.demo})),
    )
}
fn authorized(s: &ApiState, h: &HeaderMap) -> bool {
    http_authorization::browser_origin(h, s.access.port)
        && h.get("authorization")
            .is_some_and(|v| s.browser.accepts(v.as_bytes()))
}
async fn disconnect(State(s): State<ApiState>, headers: HeaderMap) -> Response {
    if !authorized(&s, &headers) {
        return http_response::error(StatusCode::UNAUTHORIZED, Error::Unauthorized);
    }
    s.browser
        .revoke(headers.get("authorization").unwrap().as_bytes());
    http_response::result(Ok(serde_json::json!({"disconnected":true})))
}
async fn command(
    State(s): State<ApiState>,
    headers: HeaderMap,
    body: std::result::Result<Json<Request>, JsonRejection>,
) -> Response {
    if !authorized(&s, &headers) {
        return http_response::error(StatusCode::UNAUTHORIZED, Error::Unauthorized);
    }
    let Ok(Json(request)) = body else {
        return http_response::error(StatusCode::BAD_REQUEST, Error::InvalidRequest);
    };
    if !BrowserAccess::permits(&request) {
        return http_response::error(StatusCode::FORBIDDEN, Error::Unauthorized);
    }
    http_response::result(runtime_mailbox::invoke(&s.commands, request).await)
}
