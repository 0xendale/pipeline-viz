//! The dashboard bundle, compiled into the binary.
//!
//! `build.rs` fills `assets/` before this module is compiled, either by running
//! the Vite build or — in the published crate — by shipping the bytes directly.

use axum::http::{header, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "assets/"]
struct Dashboard;

/// The entry document, served for `/` and for any path the bundle does not
/// claim so that client-side routes survive a reload.
const INDEX: &str = "index.html";

/// Serves `index.html`.
pub(crate) async fn index() -> Response {
    serve(INDEX)
}

/// Serves a bundled file, falling back to `index.html`.
pub(crate) async fn asset(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    if path.is_empty() {
        return serve(INDEX);
    }
    match Dashboard::get(path) {
        Some(_) => serve(path),
        // Unknown paths are dashboard routes, not missing files. Hashed bundle
        // assets are the exception — a miss there is a genuine 404, and handing
        // back HTML for a `.js` request produces a baffling parse error in the
        // console instead of a clear one in the network tab.
        None if path.starts_with("assets/") => (StatusCode::NOT_FOUND, "not found").into_response(),
        None => serve(INDEX),
    }
}

fn serve(path: &str) -> Response {
    let Some(file) = Dashboard::get(path) else {
        // Only reachable if `assets/` was built without an index.html, which
        // build.rs rejects. Degrade to a readable message rather than a panic.
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            "pipeline-viz: the dashboard bundle is missing from this binary",
        )
            .into_response();
    };

    let mime = mime_guess::from_path(path).first_or_octet_stream();

    // Every filename except index.html carries a content hash, so the bundle is
    // immutable and the document must never be cached.
    let cache = if path == INDEX {
        "no-cache"
    } else {
        "public, max-age=31536000, immutable"
    };

    (
        [
            (header::CONTENT_TYPE, mime.as_ref()),
            (header::CACHE_CONTROL, cache),
        ],
        file.data,
    )
        .into_response()
}
