//! Loopback HTTP transport, with private-file credentials and no browser access yet.
use crate::{local_access::Access, private_storage::Root, services::*};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Request as HttpRequest, State, rejection::JsonRejection},
    http::{HeaderMap, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response as HttpResponse},
    routing::post,
};
use serde::Deserialize;
use tokio::sync::{mpsc, oneshot};

pub type Envelope = (Request, oneshot::Sender<Result<Response>>);
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
fn api_error(status: StatusCode, error: Error) -> HttpResponse {
    (status, Json(serde_json::json!({"error":{"code":error}}))).into_response()
}
fn authorized(headers: &HeaderMap, access: &Access) -> bool {
    if headers.contains_key("origin")
        || headers.get("host").and_then(|h| h.to_str().ok())
            != Some(format!("127.0.0.1:{}", access.port).as_str())
    {
        return false;
    }
    let Some(actual) = headers.get("authorization").map(|h| h.as_bytes()) else {
        return false;
    };
    access.accepts(actual)
}
async fn authorize(
    State(state): State<ApiState>,
    request: HttpRequest,
    next: Next,
) -> HttpResponse {
    if !authorized(request.headers(), &state.access) {
        return api_error(StatusCode::UNAUTHORIZED, Error::Unauthorized);
    }
    next.run(request).await
}
async fn handle(
    State(state): State<ApiState>,
    body: std::result::Result<Json<Request>, JsonRejection>,
) -> HttpResponse {
    let Ok(Json(request)) = body else {
        return api_error(StatusCode::BAD_REQUEST, Error::InvalidRequest);
    };
    let (tx, rx) = oneshot::channel();
    if state.commands.send((request, tx)).await.is_err() {
        return api_error(StatusCode::SERVICE_UNAVAILABLE, Error::RuntimeUnavailable);
    }
    match rx.await {
        Ok(Ok(response)) => Json(response).into_response(),
        Ok(Err(error)) => api_error(
            match error {
                Error::NotFound => StatusCode::NOT_FOUND,
                Error::AlreadyExists | Error::RecoveryRequired => StatusCode::CONFLICT,
                Error::InvalidRequest => StatusCode::BAD_REQUEST,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            },
            error,
        ),
        Err(_) => api_error(StatusCode::SERVICE_UNAVAILABLE, Error::RuntimeUnavailable),
    }
}
pub async fn call(root: &Root, request: &Request) -> Result<Response> {
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
