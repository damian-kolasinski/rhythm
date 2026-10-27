//! Stable JSON error responses; source values and OS diagnostics never escape here.
use crate::operation_error::{Error, Result};
use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;
pub fn error(status: StatusCode, code: Error) -> Response {
    (status, Json(serde_json::json!({"error":{"code":code}}))).into_response()
}
pub fn result<T: Serialize>(value: Result<T>) -> Response {
    match value {
        Ok(v) => Json(v).into_response(),
        Err(e) => error(
            match e {
                Error::Unauthorized => StatusCode::UNAUTHORIZED,
                Error::NotFound => StatusCode::NOT_FOUND,
                Error::AlreadyExists | Error::RecoveryRequired | Error::Busy => {
                    StatusCode::CONFLICT
                }
                Error::InvalidRequest => StatusCode::BAD_REQUEST,
                Error::RuntimeUnavailable => StatusCode::SERVICE_UNAVAILABLE,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            },
            e,
        ),
    }
}
