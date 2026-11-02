use axum::http::{
    header::{COOKIE, HOST, ORIGIN},
    uri::Authority,
    HeaderMap, HeaderName, StatusCode,
};
use url::{Host, Url};

use super::{
    errors::{ApiError, ApiResult},
    principal::Principal,
    state::ApiState,
};

pub const COOKIE_NAME: &str = "albert_auth";
pub const CSRF_HEADER: &str = "x-albert-csrf";

pub fn cookie_token(headers: &HeaderMap) -> Option<String> {
    let mut matches = headers
        .get_all(COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|v| v.trim().split_once('='))
        .filter(|(key, _)| *key == COOKIE_NAME);
    let token = matches.next()?.1;
    if matches.next().is_some() {
        return None;
    }
    Some(token.into())
}
pub async fn principal(state: &ApiState, headers: &HeaderMap) -> ApiResult<Principal> {
    state
        .auth
        .authenticate(&cookie_token(headers).ok_or_else(ApiError::unauthorized)?)
        .await
        .map_err(Into::into)
}
fn single_header(headers: &HeaderMap, name: HeaderName) -> ApiResult<&str> {
    let mut values = headers.get_all(name).iter();
    let value = values.next().ok_or_else(ApiError::forbidden)?;
    if values.next().is_some() {
        return Err(ApiError::forbidden());
    }
    value.to_str().map_err(|_| ApiError::forbidden())
}

fn validated_origin(configured: &str, headers: &HeaderMap) -> ApiResult<Url> {
    let presented = single_header(headers, ORIGIN)?;
    let origin = Url::parse(presented).map_err(|_| ApiError::forbidden())?;
    if !matches!(origin.scheme(), "http" | "https")
        || origin.origin().ascii_serialization() != presented
    {
        return Err(ApiError::forbidden());
    }
    let expected = if configured.is_empty() {
        let host = single_header(headers, HOST)?;
        if host.is_empty()
            || host.ends_with(':')
            || host.bytes().any(|byte| {
                !byte.is_ascii_graphic()
                    || matches!(byte, b'@' | b'/' | b'\\' | b'?' | b'#' | b',' | b'%')
            })
        {
            return Err(ApiError::forbidden());
        }
        let authority: Authority = host.parse().map_err(|_| ApiError::forbidden())?;
        if authority.host().is_empty()
            || (authority.port().is_some() && authority.port_u16().is_none())
            || authority.port_u16() == Some(0)
        {
            return Err(ApiError::forbidden());
        }
        // Origin is browser-controlled. Match its authority against the actual
        // Host without trusting proxy headers or requiring a public URL setting.
        Url::parse(&format!("{}://{authority}", origin.scheme()))
            .map_err(|_| ApiError::forbidden())?
            .origin()
            .ascii_serialization()
    } else {
        configured.to_owned()
    };
    if presented != expected {
        return Err(ApiError::forbidden());
    }
    Ok(origin)
}

pub fn same_origin(state: &ApiState, headers: &HeaderMap) -> ApiResult<()> {
    validated_origin(&state.auth.settings().public_origin, headers).map(|_| ())
}

pub fn secure_request(state: &ApiState, headers: &HeaderMap) -> ApiResult<bool> {
    validated_origin(&state.auth.settings().public_origin, headers)
        .map(|origin| origin.scheme() == "https")
}

pub fn login_origin(
    state: &ApiState,
    headers: &HeaderMap,
    username: &str,
    password: &str,
) -> ApiResult<()> {
    let configured = &state.auth.settings().public_origin;
    let origin = validated_origin(configured, headers)?;
    if configured.is_empty() && username.eq_ignore_ascii_case("owner") && password == "owner" {
        // HTTP DNS rebinding must not claim the known bootstrap account.
        // A browser's same-authority HTTPS request also works through TLS termination.
        let direct = matches!(origin.host(), Some(Host::Ipv4(_) | Host::Ipv6(_)))
            || origin.host_str() == Some("localhost")
            || origin.scheme() == "https";
        if !direct {
            return Err(ApiError::new(StatusCode::FORBIDDEN, "initial_setup_origin",
                "Complete initial setup through localhost, an IP address or HTTPS. After changing the initial password, you can also use HTTP hostnames."));
        }
    }
    Ok(())
}
pub fn csrf(state: &ApiState, headers: &HeaderMap, principal: &Principal) -> ApiResult<()> {
    same_origin(state, headers)?;
    let presented = headers
        .get(CSRF_HEADER)
        .and_then(|h| h.to_str().ok())
        .ok_or_else(ApiError::forbidden)?;
    let expected = principal.csrf();
    if presented.len() != expected.len()
        || presented
            .as_bytes()
            .iter()
            .zip(expected.as_bytes())
            .fold(0u8, |a, (x, y)| a | (x ^ y))
            != 0
    {
        return Err(ApiError::forbidden());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use axum::{
        body::Body,
        http::{
            header::{HOST, ORIGIN, SET_COOKIE},
            HeaderMap, Method, Request, StatusCode,
        },
        Router,
    };
    use serde_json::json;
    use tower::ServiceExt;

    use super::validated_origin;
    use crate::api::testing::{cookie, fixture_with_origin, request};

    fn headers(host: &str, origin: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(HOST, host.parse().unwrap());
        headers.insert(ORIGIN, origin.parse().unwrap());
        headers
    }

    #[test]
    fn automatic_http_origins_match_strict_hostname_ip_ipv6_and_default_port_authorities() {
        for (host, origin) in [
            ("localhost:9877", "http://localhost:9877"),
            ("192.0.2.40:9877", "http://192.0.2.40:9877"),
            ("[2001:db8::40]:9877", "http://[2001:db8::40]:9877"),
            ("Workspace.Example:80", "http://workspace.example"),
            ("workspace.example:443", "https://workspace.example"),
            ("localhost:9877", "https://localhost:9877"),
        ] {
            assert!(
                validated_origin("", &headers(host, origin)).is_ok(),
                "{host}"
            );
        }
        for origin in [
            "http://other.example:9877",
            "http://localhost:9878",
            "null",
            "http://localhost:9877/",
            "http://user@localhost:9877",
            "http://localhost:9877 http://other.example",
        ] {
            assert!(
                validated_origin("", &headers("localhost:9877", origin)).is_err(),
                "{origin}"
            );
        }
    }

    #[test]
    fn malformed_missing_and_duplicate_authority_headers_are_rejected() {
        for host in [
            "",
            "localhost:",
            "localhost:abc",
            "localhost:65536",
            "localhost:0",
            "user@localhost:9877",
            "localhost:9877/path",
            "localhost:9877?query",
            "localhost:9877#fragment",
            "localhost:9877,other.example",
            "local%68ost:9877",
            "local host:9877",
        ] {
            assert!(
                validated_origin("", &headers(host, "http://localhost:9877")).is_err(),
                "{host}"
            );
        }
        let mut missing = headers("localhost:9877", "http://localhost:9877");
        missing.remove(HOST);
        assert!(validated_origin("", &missing).is_err());
        let mut missing = headers("localhost:9877", "http://localhost:9877");
        missing.remove(ORIGIN);
        assert!(validated_origin("", &missing).is_err());
        for name in [HOST, ORIGIN] {
            let mut duplicate = headers("localhost:9877", "http://localhost:9877");
            let value = duplicate.get(&name).unwrap().clone();
            duplicate.append(name, value);
            assert!(validated_origin("", &duplicate).is_err());
        }
    }

    #[test]
    fn forwarded_headers_do_not_override_transport_and_explicit_https_can_override_proxy_host() {
        let mut headers = headers("internal.example:9877", "https://workspace.example");
        headers.insert("x-forwarded-host", "workspace.example".parse().unwrap());
        headers.insert("x-forwarded-proto", "https".parse().unwrap());
        headers.insert(
            "forwarded",
            "proto=https;host=workspace.example".parse().unwrap(),
        );
        assert!(validated_origin("", &headers).is_err());
        assert!(validated_origin("https://workspace.example", &headers).is_ok());
        assert!(validated_origin("https://other.example", &headers).is_err());
        headers.append(ORIGIN, "https://workspace.example".parse().unwrap());
        assert!(validated_origin("https://workspace.example", &headers).is_err());
    }

    async fn login(
        router: &Router,
        host: &str,
        origin: &str,
        username: &str,
        password: &str,
    ) -> (StatusCode, HeaderMap) {
        let response = router
            .clone()
            .oneshot(
                Request::post("/api/v1/auth/credentials")
                    .header(HOST, host)
                    .header(ORIGIN, origin)
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({"username":username,"password":password}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        (response.status(), response.headers().clone())
    }

    #[tokio::test]
    async fn automatic_bootstrap_is_direct_only_then_rotated_domain_login_and_csrf_work() {
        let fixture = fixture_with_origin("").await;
        for host in [
            "workspace.example:9877",
            "localhost.attacker.example:9877",
            "localhost.:9877",
            "127.0.0.1.attacker.example:9877",
        ] {
            let (status, headers) = login(
                &fixture.router,
                host,
                &format!("http://{host}"),
                "OWNER",
                "owner",
            )
            .await;
            assert_eq!(status, StatusCode::FORBIDDEN);
            assert!(!headers.contains_key(SET_COOKIE));
        }
        for host in ["localhost:9877", "192.0.2.40:9877", "[2001:db8::40]:9877"] {
            let (status, headers) = login(
                &fixture.router,
                host,
                &format!("http://{host}"),
                "owner",
                "owner",
            )
            .await;
            assert_eq!(status, StatusCode::OK);
            assert!(headers.contains_key(SET_COOKIE));
        }
        let (_, headers) = login(
            &fixture.router,
            "localhost:9877",
            "http://localhost:9877",
            "owner",
            "owner",
        )
        .await;
        let old_cookie = cookie(&headers);
        let (_, _, context) = request(
            &fixture.router,
            Method::GET,
            "/api/v1/context",
            Some(&old_cookie),
            None,
            None,
            json!({}),
        )
        .await;
        let (status, _, _) = request(
            &fixture.router,
            Method::PUT,
            "/api/v1/account/credentials",
            Some(&old_cookie),
            None,
            Some("http://localhost:9877"),
            json!({"current_password":"owner","new_password":"rotated-test-password"}),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _, _) = request(
            &fixture.router,
            Method::PUT,
            "/api/v1/account/credentials",
            Some(&old_cookie),
            context["csrf"].as_str(),
            Some("http://localhost:9877"),
            json!({"current_password":"owner","new_password":"rotated-test-password"}),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let (status, headers) = login(
            &fixture.router,
            "workspace.example:9877",
            "http://workspace.example:9877",
            "owner",
            "rotated-test-password",
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(headers.contains_key(SET_COOKIE));
    }

    #[tokio::test]
    async fn explicit_https_origin_keeps_proxy_bootstrap_and_secure_cookies() {
        let fixture = fixture_with_origin("https://workspace.example").await;
        let (status, headers) = login(
            &fixture.router,
            "internal.example:9877",
            "https://workspace.example",
            "owner",
            "owner",
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(headers[SET_COOKIE].to_str().unwrap().contains("Secure"));
        let (status, _) = login(
            &fixture.router,
            "internal.example:9877",
            "http://workspace.example",
            "owner",
            "owner",
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn matching_https_origin_works_without_a_public_url_setting() {
        let fixture = fixture_with_origin("").await;
        let (status, headers) = login(
            &fixture.router,
            "workspace.example",
            "https://workspace.example",
            "owner",
            "owner",
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(headers[SET_COOKIE].to_str().unwrap().contains("; Secure"));
        let (status, _) = login(
            &fixture.router,
            "other.example",
            "https://workspace.example",
            "owner",
            "owner",
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }
}
