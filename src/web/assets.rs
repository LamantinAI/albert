use axum::{
    body::Body,
    http::{
        header::{CACHE_CONTROL, CONTENT_TYPE},
        StatusCode, Uri,
    },
    response::{IntoResponse, Redirect, Response},
    routing::get,
    Router,
};
use include_dir::{include_dir, Dir};
use mime_guess::from_path;

static ASSETS: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/ui-dist");

pub(crate) fn router() -> Router {
    Router::new()
        .route("/", get(|| async { Redirect::temporary("/ui/") }))
        .route("/ui", get(|| async { Redirect::permanent("/ui/") }))
        .route("/ui/", get(asset))
        .route("/ui/{*path}", get(asset))
}

async fn asset(uri: Uri) -> Response {
    let path = uri.path().strip_prefix("/ui/").unwrap_or("");
    if path.contains("..") || path.contains('\\') || path.contains('%') {
        return StatusCode::NOT_FOUND.into_response();
    }
    let file = ASSETS
        .get_file(if path.is_empty() { "index.html" } else { path })
        .or_else(|| {
            (!path.contains('.'))
                .then(|| ASSETS.get_file("index.html"))
                .flatten()
        });
    let Some(file) = file else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let extension = file
        .path()
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    if !["html", "js", "css", "svg", "png", "ico", "woff2", "txt"].contains(&extension) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let cache = if extension == "html" {
        "no-store"
    } else if file.path().to_string_lossy().starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "public, max-age=3600"
    };
    let policy = if path == "docs/index.html" {
        "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self'; connect-src 'self'; base-uri 'none'; frame-ancestors 'none'; object-src 'none'; form-action 'self'"
    } else {
        "default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data: https:; font-src 'self'; connect-src 'self'; base-uri 'none'; frame-ancestors 'none'; object-src 'none'; form-action 'self'"
    };
    Response::builder()
        .header(
            CONTENT_TYPE,
            from_path(file.path()).first_or_octet_stream().as_ref(),
        )
        .header(CACHE_CONTROL, cache)
        .header("x-content-type-options", "nosniff")
        .header("content-security-policy", policy)
        .body(Body::from(file.contents()))
        .expect("static response")
}
