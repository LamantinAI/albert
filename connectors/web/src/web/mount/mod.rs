//! A workspace is self-contained beneath its document URL. Prefixes preserved
//! by a proxy are removed only around the application's own route boundaries.
use axum::{
    extract::Request, extract::State, http::header::ACCEPT, http::Method, http::StatusCode,
    http::Uri, response::IntoResponse, response::Response, Router,
};
use tower::ServiceExt;

use crate::api::docs::ROUTES;

pub(super) fn router(routes: Router) -> Router {
    Router::new().fallback(dispatch).with_state(routes)
}

async fn dispatch(State(routes): State<Router>, mut request: Request) -> Response {
    let path = request.uri().path();
    if !safe_path(path) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let document = matches!(*request.method(), Method::GET | Method::HEAD)
        && request
            .headers()
            .get(ACCEPT)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| {
                value
                    .split(',')
                    .any(|part| part.trim().starts_with("text/html"))
            });
    let canonical = canonical_path(path, document).or_else(|| {
        (matches!(*request.method(), Method::GET | Method::HEAD)
            && (document || !path.rsplit('/').next().unwrap_or("").contains('.')))
        .then_some("/")
    });
    let Some(canonical) = canonical else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let path_query = match request.uri().query() {
        Some(query) => format!("{canonical}?{query}"),
        None => canonical.to_owned(),
    };
    let mut parts = request.uri().clone().into_parts();
    let Ok(path_query) = path_query.parse() else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    parts.path_and_query = Some(path_query);
    let Ok(uri) = Uri::from_parts(parts) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    *request.uri_mut() = uri;
    match routes.oneshot(request).await {
        Ok(response) => response,
        Err(never) => match never {},
    }
}

fn boundary<'a>(path: &'a str, marker: &str) -> Option<&'a str> {
    path.rmatch_indices(marker).find_map(|(offset, _)| {
        let suffix = &path[offset..];
        (suffix.len() == marker.len() || suffix.as_bytes().get(marker.len()) == Some(&b'/'))
            .then_some(suffix)
    })
}

fn api_route(path: &str) -> bool {
    let tail = path.strip_prefix("/api/v1").unwrap_or(path);
    tail == "/openapi.json"
        || ROUTES.iter().any(|(_, pattern, _, _)| {
            let expected: Vec<_> = pattern.split('/').collect();
            let actual: Vec<_> = tail.split('/').collect();
            expected.len() == actual.len()
                && expected.iter().zip(actual).all(|(expected, actual)| {
                    if expected.starts_with('{') {
                        !actual.is_empty()
                    } else {
                        *expected == actual
                    }
                })
        })
}

fn canonical_path(path: &str, document: bool) -> Option<&str> {
    let api = boundary(path, "/api/v1");
    let ui = boundary(path, "/ui");
    if api.is_none_or(|api| !api_route(api)) {
        if let Some(openapi) =
            boundary(path, "/openapi.json").filter(|path| *path == "/openapi.json")
        {
            if ui.is_none_or(|ui| !workspace_document(ui)) {
                return Some(openapi);
            }
        }
        if let Some(docs) = boundary(path, "/docs") {
            if ui.is_none_or(|ui| !workspace_document(ui)) {
                return Some(docs);
            }
        }
    }
    if document && !path.ends_with("/openapi.json") {
        return ui.filter(|path| workspace_document(path)).or(Some("/"));
    }
    if let Some(api) = api {
        if ui.is_none_or(|ui| api.len() < ui.len()) || api_route(api) {
            return Some(api);
        }
    }
    ui.or(api)
}

fn workspace_document(path: &str) -> bool {
    let page = path
        .trim_start_matches("/ui")
        .trim_start_matches('/')
        .split('/')
        .next()
        .unwrap_or("");
    matches!(
        page,
        "" | "setup"
            | "chats"
            | "workspace"
            | "account"
            | "models"
            | "skills"
            | "settings"
            | "approvals"
            | "users"
            | "docs"
            | "index.html"
    )
}

fn safe_path(path: &str) -> bool {
    if !path.starts_with('/') || path.contains("//") || path.contains('\\') {
        return false;
    }
    for part in path.split('/') {
        let mut decoded = Vec::new();
        let mut bytes = part.as_bytes().iter().copied();
        while let Some(byte) = bytes.next() {
            if byte == b'%' {
                let (Some(a), Some(b)) = (bytes.next(), bytes.next()) else {
                    return false;
                };
                let (Some(a), Some(b)) = ((a as char).to_digit(16), (b as char).to_digit(16))
                else {
                    return false;
                };
                decoded.push((a * 16 + b) as u8);
            } else {
                decoded.push(byte);
            }
        }
        if decoded == b"."
            || decoded == b".."
            || decoded
                .iter()
                .any(|byte| byte.is_ascii_control() || matches!(*byte, b'/' | b'\\'))
        {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use axum::{
        body::to_bytes, body::Body, http::header::ACCEPT, http::header::CONTENT_SECURITY_POLICY,
        http::header::LOCATION, http::Request, http::StatusCode,
    };
    use tower::ServiceExt;

    use super::{canonical_path, router as mounted_router, safe_path};
    #[cfg(feature = "embedded-panel")]
    use crate::web::assets::router as assets_router;

    #[test]
    fn identifies_namespaces_without_configuring_proxy_prefixes() {
        assert_eq!(
            canonical_path("/tools/agent/ui/chats/id", true),
            Some("/ui/chats/id")
        );
        assert_eq!(
            canonical_path("/tools/ui/agent/ui/chats/id", true),
            Some("/ui/chats/id")
        );
        assert_eq!(
            canonical_path("/tools/agent/ui/api/v1/context", false),
            Some("/api/v1/context")
        );
        assert_eq!(
            canonical_path("/api/v1/models/ui", false),
            Some("/api/v1/models/ui")
        );
        assert_eq!(
            canonical_path("/tools/api/v1/agent/ui/", true),
            Some("/ui/")
        );
        for path in [
            "/api/v1/sessions/docs",
            "/api/v1/configuration/openapi.json",
        ] {
            assert_eq!(canonical_path(path, false), Some(path));
        }
        assert_eq!(
            canonical_path("/tools/ui/docs-agent/docs/", true),
            Some("/docs/")
        );
        for path in ["/ui/chats/docs", "/ui/chats/openapi.json"] {
            assert_eq!(canonical_path(path, true), Some(path));
        }
        assert_eq!(canonical_path("/tools/agent", true), Some("/"));
        assert_eq!(canonical_path("/ui/agents/api/v1", true), Some("/"));
        assert_eq!(
            canonical_path("/tools/agent/ui/docs/index.html", true),
            Some("/ui/docs/index.html")
        );
        assert_eq!(
            canonical_path("/tools/ui/api/v1/not-a-route", false),
            Some("/api/v1/not-a-route")
        );
    }

    #[test]
    fn rejects_ambiguous_or_traversing_paths_but_keeps_encoded_identifiers() {
        for path in [
            "//other.example/ui",
            "/a/../ui",
            "/a/%2e%2e/ui",
            "/a/%2f/ui",
            "/a/%5c/ui",
            "/a/%00/ui",
            "/a/%zz/ui",
        ] {
            assert!(!safe_path(path), "{path}");
        }
        assert!(safe_path("/tools/%D0%B0/ui/chats/id"));
        assert!(safe_path("/api/v1/configuration/connector%3Aexample"));
    }

    #[cfg(feature = "embedded-panel")]
    #[tokio::test]
    async fn nested_workspace_deep_refresh_keeps_relative_assets_and_strict_csp() {
        let router = mounted_router(assets_router());
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/tools/assistant/ui/chats/example")
                    .header(ACCEPT, "text/html")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.headers()[CONTENT_SECURITY_POLICY]
            .to_str()
            .unwrap()
            .contains("base-uri 'none'"));
        let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        let html = String::from_utf8(bytes.to_vec()).unwrap();
        assert!(html.contains("name=\"albert-ui-root\" content=\"../\""));
        assert!(html.contains("../assets/"));
        assert!(!html.contains("/tools/assistant"));
        let missing = router
            .oneshot(
                Request::builder()
                    .uri("/tools/assistant/ui/assets/missing.js")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    }

    #[cfg(feature = "embedded-panel")]
    #[tokio::test]
    async fn entry_points_do_not_need_forwarded_prefix_or_inline_script_permission() {
        let router = mounted_router(assets_router());
        for uri in ["/", "/tools/assistant", "/tools/assistant/"] {
            let response = router
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(uri)
                        .header(ACCEPT, "text/html")
                        .header("x-forwarded-prefix", "//untrusted.invalid")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let csp = response.headers()[CONTENT_SECURITY_POLICY]
                .to_str()
                .unwrap();
            assert!(csp.contains("'sha256-"));
            assert!(!csp.contains("unsafe-inline"));
            let html =
                String::from_utf8(to_bytes(response.into_body(), 4096).await.unwrap().to_vec())
                    .unwrap();
            assert!(html.contains("window.location.href"));
            assert!(!html.contains("untrusted.invalid"));
        }
        let response = router
            .oneshot(
                Request::builder()
                    .uri("/tools/assistant/ui?new=temporary")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.headers()[LOCATION], "ui/?new=temporary");
    }
    #[cfg(feature = "embedded-panel")]
    #[tokio::test]
    async fn standalone_docs_and_schema_aliases_keep_proxy_prefixes_local() {
        use crate::api::docs::schema;
        use axum::{routing::get, Json};

        let router = mounted_router(
            assets_router().route("/openapi.json", get(|| async { Json(schema()) })),
        );
        for path in [
            "/docs/",
            "/docs/index.html",
            "/tools/agent/docs/",
            "/tools/agent/docs/index.html",
            "/ui/docs/index.html",
            "/tools/agent/ui/docs/index.html",
        ] {
            let response = router
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(path)
                        .header(ACCEPT, "text/html")
                        .header("host", "untrusted.invalid")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK, "{path}");
            let policy = response.headers()[CONTENT_SECURITY_POLICY]
                .to_str()
                .unwrap();
            assert!(policy.contains("script-src 'self'"));
            assert!(!policy.contains("script-src 'unsafe-inline'"));
            let html = String::from_utf8(
                to_bytes(response.into_body(), 16384)
                    .await
                    .unwrap()
                    .to_vec(),
            )
            .unwrap();
            assert!(html.contains("href=\"./swagger-ui.css\""));
            assert!(html.contains("src=\"./bootstrap.js\""));
            assert!(!html.contains("albert-docs.css"));
            assert!(!html.contains("untrusted.invalid"));
        }
        for path in ["/docs", "/tools/agent/docs"] {
            let response = router
                .clone()
                .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.headers()[LOCATION], "docs/");
        }
        for path in [
            "/docs/bootstrap.js",
            "/tools/agent/docs/bootstrap.js",
            "/ui/docs/bootstrap.js",
        ] {
            let response = router
                .clone()
                .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let script = String::from_utf8(
                to_bytes(response.into_body(), 16384)
                    .await
                    .unwrap()
                    .to_vec(),
            )
            .unwrap();
            assert!(script.contains("../api/v1"));
            assert!(script.contains("x-albert-csrf"));
            assert!(script.contains("same-origin"));
        }
        for path in ["/openapi.json", "/tools/agent/openapi.json"] {
            let response = router
                .clone()
                .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
        }
    }
}
