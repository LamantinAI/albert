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
use base64::{engine::general_purpose::STANDARD, Engine};
use include_dir::{include_dir, Dir};
use mime_guess::from_path;
use sha2::{Digest, Sha256};

static ASSETS: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/ui-dist");

const LANDING_SCRIPT: &str = r#"(()=>{const u=new URL(window.location.href);u.pathname=u.pathname.replace(/\/+$/,"")+"/ui/";window.location.replace(u.href)})()"#;

pub(crate) fn router() -> Router {
    Router::new()
        .route("/", get(landing))
        .route(
            "/ui",
            get(|uri: Uri| async move {
                let target = match uri.query() {
                    Some(query) => format!("ui/?{query}"),
                    None => "ui/".into(),
                };
                Redirect::permanent(&target)
            }),
        )
        .route("/ui/", get(asset))
        .route("/ui/{*path}", get(asset))
}

async fn landing() -> Response {
    let digest = STANDARD.encode(Sha256::digest(LANDING_SCRIPT.as_bytes()));
    Response::builder().header(CONTENT_TYPE, "text/html; charset=utf-8")
        .header(CACHE_CONTROL, "no-store")
        .header("content-security-policy", format!("default-src 'none'; script-src 'sha256-{digest}'; base-uri 'none'; frame-ancestors 'none'; form-action 'self'"))
        .header("x-content-type-options", "nosniff")
        .body(Body::from(format!("<!doctype html><html><head><meta charset=\"utf-8\"><title>Albert</title></head><body><script>{LANDING_SCRIPT}</script><noscript>JavaScript is required to open this workspace.</noscript></body></html>")))
        .expect("static landing response")
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
        .body(if extension == "html" {
            let Some(html) = file.contents_utf8() else {
                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
            };
            Body::from(render_html(html, path))
        } else {
            Body::from(file.contents())
        })
        .expect("static response")
}

fn render_html(html: &str, path: &str) -> String {
    let depth = path.bytes().filter(|byte| *byte == b'/').count();
    let relative = if depth == 0 {
        "./".to_owned()
    } else {
        "../".repeat(depth)
    };
    // Only a relative distance is rendered. No Host or forwarding header is
    // reflected into executable content, resource URLs or trusted API state.
    html.replace(
        "<head>",
        &format!("<head><meta name=\"albert-ui-root\" content=\"{relative}\">"),
    )
    .replace("=\"/ui/", &format!("=\"{relative}"))
    .replace("=\"./ui/", &format!("=\"{relative}"))
    .replace("=\"/api/v1/", &format!("=\"{relative}api/v1/"))
    .replace("=\"./assets/", &format!("=\"{relative}assets/"))
    .replace("=\"./favicon.svg", &format!("=\"{relative}favicon.svg"))
}
