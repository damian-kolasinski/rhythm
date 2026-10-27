//! Explicit loopback Host/Origin policy shared by browser and CLI transports.
use axum::http::HeaderMap;
pub fn host(headers: &HeaderMap, port: u16) -> bool {
    headers.get_all("host").iter().count() == 1
        && headers.get("host").and_then(|h| h.to_str().ok())
            == Some(format!("127.0.0.1:{port}").as_str())
}
pub fn browser_origin(headers: &HeaderMap, port: u16) -> bool {
    host(headers, port)
        && headers.get_all("origin").iter().count() == 1
        && headers.get("origin").and_then(|h| h.to_str().ok())
            == Some(format!("http://127.0.0.1:{port}").as_str())
}
pub fn cli_origin(headers: &HeaderMap, port: u16) -> bool {
    host(headers, port) && !headers.contains_key("origin")
}
