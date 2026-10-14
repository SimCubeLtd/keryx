//! The dashboard is built ahead of Cargo and embedded in every executable.
use axum::{
    extract::Path,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
};

include!(concat!(env!("OUT_DIR"), "/frontend.rs"));

fn asset(path: &str) -> Response {
    let Some((_, bytes)) = ASSETS.iter().find(|(name, _)| *name == path) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let content_type = match path.rsplit('.').next() {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("woff2") => "font/woff2",
        Some("png") => "image/png",
        _ => "application/octet-stream",
    };
    ([(header::CONTENT_TYPE, content_type)], *bytes).into_response()
}

pub async fn dashboard() -> Response {
    asset("index.html")
}
pub async fn javascript_asset(Path(path): Path<String>) -> Response {
    asset(&format!("assets/{path}"))
}
