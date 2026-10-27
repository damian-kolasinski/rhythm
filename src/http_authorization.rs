//! Explicit loopback Host/Origin policy shared by browser and CLI transports.
use axum::http::HeaderMap;
pub const BROWSER_CLIENT_HEADER: &str = "x-rhythm-client";
pub fn host(headers: &HeaderMap, port: u16) -> bool {
    headers.get_all("host").iter().count() == 1
        && headers
            .get("host")
            .and_then(|h| h.to_str().ok())
            .is_some_and(|host| {
                host == format!("127.0.0.1:{port}") || host == format!("localhost:{port}")
            })
}
pub fn browser_request(headers: &HeaderMap, port: u16) -> bool {
    host(headers, port)
        && headers.get_all("origin").iter().count() == 1
        && headers
            .get("origin")
            .and_then(|h| h.to_str().ok())
            .is_some_and(|origin| {
                Some(origin)
                    == headers
                        .get("host")
                        .and_then(|h| h.to_str().ok())
                        .map(|h| format!("http://{h}"))
                        .as_deref()
            })
        && headers.get_all(BROWSER_CLIENT_HEADER).iter().count() == 1
        && headers.get(BROWSER_CLIENT_HEADER).is_some_and(|v| v == "1")
}
pub fn cli_origin(headers: &HeaderMap, port: u16) -> bool {
    host(headers, port) && !headers.contains_key("origin")
}
