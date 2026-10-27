//! Fixed embedded assets: no filesystem routing or path traversal.
use axum::{
    Router,
    http::{StatusCode, header},
    response::IntoResponse,
    routing::get,
};
pub fn router() -> Router {
    Router::new()
        .route(
            "/",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
                    include_bytes!("../web/dist/index.html").as_slice(),
                )
            }),
        )
        .route(
            "/assets/app.js",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
                    include_bytes!("../web/dist/assets/app.js").as_slice(),
                )
            }),
        )
        .route(
            "/assets/app.css",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
                    include_bytes!("../web/dist/assets/app.css").as_slice(),
                )
            }),
        )
        .fallback(|| async { StatusCode::NOT_FOUND.into_response() })
}
