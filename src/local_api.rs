//! CLI loopback transport, using private-file credentials and rejecting browser origins.
use crate::{
    http_authorization, http_response,
    local_access::Access,
    operation_error::{Error, Result},
    private_storage::Root,
    runtime_mailbox,
    runtime_protocol::{Command, Request},
};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Request as HttpRequest, State, rejection::JsonRejection},
    http::{HeaderMap, StatusCode},
    middleware::{self, Next},
    response::Response as HttpResponse,
    routing::post,
};
use serde::Deserialize;
use tokio::sync::mpsc;

pub use crate::runtime_mailbox::Envelope;
#[derive(Clone)]
struct ApiState {
    access: Access,
    commands: mpsc::Sender<Envelope>,
}
pub fn router(access: Access, commands: mpsc::Sender<Envelope>) -> Router {
    let state = ApiState { access, commands };
    Router::new()
        .route("/v1/command", post(handle))
        .layer(DefaultBodyLimit::max(131072))
        .layer(middleware::from_fn_with_state(state.clone(), authorize))
        .with_state(state)
}
fn authorized(headers: &HeaderMap, access: &Access) -> bool {
    http_authorization::cli_origin(headers, access.port)
        && headers
            .get("authorization")
            .is_some_and(|h| access.accepts(h.as_bytes()))
}
async fn authorize(
    State(state): State<ApiState>,
    request: HttpRequest,
    next: Next,
) -> HttpResponse {
    if !authorized(request.headers(), &state.access) {
        return http_response::error(StatusCode::UNAUTHORIZED, Error::Unauthorized);
    }
    next.run(request).await
}
async fn handle(
    State(state): State<ApiState>,
    body: std::result::Result<Json<Request>, JsonRejection>,
) -> HttpResponse {
    let Ok(Json(request)) = body else {
        return http_response::error(StatusCode::BAD_REQUEST, Error::InvalidRequest);
    };
    http_response::result(runtime_mailbox::invoke(&state.commands, request).await)
}

pub async fn call<C: Command>(root: &Root, request: &C) -> Result<C::Response> {
    let access = Access::read(root)?;
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(std::time::Duration::from_secs(1))
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|_| Error::RuntimeUnavailable)?;
    let response = client
        .post(format!("http://127.0.0.1:{}/v1/command", access.port))
        .bearer_auth(access.credential())
        .json(request)
        .send()
        .await
        .map_err(|_| Error::RuntimeUnavailable)?;
    if response.status().is_success() {
        response.json().await.map_err(|_| Error::RuntimeUnavailable)
    } else {
        #[derive(Deserialize)]
        struct Body {
            error: Code,
        }
        #[derive(Deserialize)]
        struct Code {
            code: Error,
        }
        response
            .json::<Body>()
            .await
            .map(|v| v.error.code)
            .map_or(Err(Error::RuntimeUnavailable), Err)
    }
}
