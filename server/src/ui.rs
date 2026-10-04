//! The web UI (`web/dist`), embedded at build time, with SPA fallback:
//! unknown paths get `index.html` so client-side routes work on reload.

use axum::{
    http::{header, HeaderValue, Method, StatusCode, Uri},
    response::{IntoResponse, Response},
};
use rust_embed::RustEmbed;

use crate::{error::AppError, registry::OciCode};

#[derive(RustEmbed)]
#[folder = "$MINREGISTRY_WEB_DIST"]
struct Assets;

pub(crate) async fn serve(method: Method, uri: Uri) -> Response {
    let path = uri.path();
    if path.starts_with("/api/") || path.starts_with("/auth/") {
        return AppError::not_found(OciCode::Unsupported, "not found").into_response();
    }
    if method != Method::GET && method != Method::HEAD {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }
    let file = path.trim_start_matches('/');
    match Assets::get(file).filter(|_| !file.is_empty()) {
        Some(asset) => {
            let mime = mime_guess::from_path(file).first_or_octet_stream();
            // Vite fingerprints everything under assets/.
            let cache = if file.starts_with("assets/") { "public, max-age=31536000, immutable" } else { "no-cache" };
            respond(asset.data.into_owned(), mime.as_ref(), cache)
        }
        None => match Assets::get("index.html") {
            Some(index) => respond(index.data.into_owned(), "text/html; charset=utf-8", "no-cache"),
            None => StatusCode::NOT_FOUND.into_response(),
        },
    }
}

fn respond(body: Vec<u8>, content_type: &str, cache: &'static str) -> Response {
    let mut res = body.into_response();
    let h = res.headers_mut();
    if let Ok(ct) = HeaderValue::from_str(content_type) {
        h.insert(header::CONTENT_TYPE, ct);
    }
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static(cache));
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    h.insert("x-frame-options", HeaderValue::from_static("DENY"));
    h.insert(header::REFERRER_POLICY, HeaderValue::from_static("same-origin"));
    res
}
