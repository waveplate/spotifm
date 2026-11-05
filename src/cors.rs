use std::convert::Infallible;
use warp::{Filter, Reply};

/// Apply CORS after route rejection recovery so API errors also get CORS headers.
pub fn with_cors<F>(
    routes: F,
    origins: &[String],
) -> impl Filter<Extract = (impl Reply,), Error = Infallible> + Clone
where
    F: Filter<Error = Infallible> + Clone + Send + Sync + 'static,
    F::Extract: Reply,
{
    let cors = warp::cors()
        .allow_methods(["GET", "HEAD", "POST", "DELETE", "OPTIONS"])
        .allow_headers(["authorization", "content-type", "x-api-key", "range"])
        .expose_headers([
            "retry-after",
            "x-spotify-source",
            "x-spotifm-ogg-granule-rate",
        ]);
    let cors = if origins.iter().any(|origin| origin == "*") {
        cors.allow_any_origin()
    } else {
        cors.allow_origins(origins.iter().map(String::as_str))
    };

    routes
        .with(cors)
        .recover(crate::handle_rejection)
        .with(warp::reply::with::header(
            "vary",
            "Origin, Access-Control-Request-Method, Access-Control-Request-Headers",
        ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use warp::http::StatusCode;

    fn routes(
        origins: &[String],
    ) -> impl Filter<Extract = (impl Reply,), Error = Infallible> + Clone {
        let ok = warp::path!("ok").and(warp::get()).map(warp::reply);
        let protected =
            warp::path!("auth" / String)
                .and(warp::get())
                .and_then(|kind: String| async move {
                    let error = match kind.as_str() {
                        "missing" => crate::AuthError::MissingKey,
                        "invalid" => crate::AuthError::InvalidKey,
                        _ => crate::AuthError::PermissionDenied,
                    };
                    Err::<warp::reply::Response, _>(warp::reject::custom(error))
                });
        with_cors(ok.or(protected).recover(crate::handle_rejection), origins)
    }

    #[tokio::test]
    async fn wildcard_allows_any_origin_and_exposes_api_and_stream_headers() {
        let routes = routes(&["*".to_string()]);
        for origin in ["https://player.example", "http://localhost:5173", "null"] {
            let response = warp::test::request()
                .path("/ok")
                .header("origin", origin)
                .reply(&routes)
                .await;
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(response.headers()["access-control-allow-origin"], origin);
            let exposed = response.headers()["access-control-expose-headers"]
                .to_str()
                .unwrap();
            for header in [
                "retry-after",
                "x-spotify-source",
                "x-spotifm-ogg-granule-rate",
            ] {
                assert!(exposed.contains(header), "{header}");
            }
            assert!(response.headers()["vary"]
                .to_str()
                .unwrap()
                .contains("Origin"));
            assert!(!response
                .headers()
                .contains_key("access-control-allow-credentials"));
        }
    }

    #[tokio::test]
    async fn preflight_allows_api_methods_and_headers_without_authentication() {
        let routes = routes(&["*".to_string()]);
        for method in ["GET", "HEAD", "POST", "DELETE"] {
            let response = warp::test::request()
                .method("OPTIONS")
                .path("/auth/missing")
                .header("origin", "https://player.example")
                .header("access-control-request-method", method)
                .header(
                    "access-control-request-headers",
                    "Authorization, X-Api-Key, Content-Type, Range",
                )
                .reply(&routes)
                .await;
            assert_eq!(response.status(), StatusCode::OK, "{method}");
            assert_eq!(
                response.headers()["access-control-allow-origin"],
                "https://player.example"
            );
            assert!(response.headers()["access-control-allow-methods"]
                .to_str()
                .unwrap()
                .split(", ")
                .any(|allowed| allowed == method));
            let headers = response.headers()["access-control-allow-headers"]
                .to_str()
                .unwrap();
            for header in ["authorization", "x-api-key", "content-type", "range"] {
                assert!(headers.contains(header), "{header}");
            }
        }
    }

    #[tokio::test]
    async fn api_errors_keep_status_and_cors_headers() {
        let routes = routes(&["*".to_string()]);
        for (method, path, status) in [
            ("GET", "/auth/missing", StatusCode::UNAUTHORIZED),
            ("GET", "/auth/invalid", StatusCode::FORBIDDEN),
            ("GET", "/auth/denied", StatusCode::FORBIDDEN),
            ("GET", "/missing", StatusCode::NOT_FOUND),
            ("POST", "/ok", StatusCode::METHOD_NOT_ALLOWED),
        ] {
            let response = warp::test::request()
                .method(method)
                .path(path)
                .header("origin", "https://player.example")
                .reply(&routes)
                .await;
            assert_eq!(response.status(), status, "{method} {path}");
            assert_eq!(
                response.headers()["access-control-allow-origin"],
                "https://player.example"
            );
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(response.body()).unwrap()["code"],
                status.as_u16()
            );
        }
    }

    #[tokio::test]
    async fn explicit_origins_allow_only_listed_origins_for_requests_and_preflights() {
        let routes = routes(&[
            "https://player.example".to_string(),
            "http://localhost:5173".to_string(),
        ]);
        for (origin, allowed) in [
            ("https://player.example", true),
            ("http://localhost:5173", true),
            ("https://other.example", false),
            ("http://localhost:5174", false),
            ("null", false),
        ] {
            for method in ["GET", "OPTIONS"] {
                let response = warp::test::request()
                    .method(method)
                    .path("/ok")
                    .header("origin", origin)
                    .header("access-control-request-method", "GET")
                    .reply(&routes)
                    .await;
                assert_eq!(
                    response.status(),
                    if allowed {
                        StatusCode::OK
                    } else {
                        StatusCode::FORBIDDEN
                    },
                    "{method} {origin}"
                );
                if allowed {
                    assert_eq!(response.headers()["access-control-allow-origin"], origin);
                } else {
                    assert!(!response
                        .headers()
                        .contains_key("access-control-allow-origin"));
                }
            }
        }
    }

    #[tokio::test]
    async fn empty_origins_reject_cross_origin_requests_and_keep_non_cors_requests_working() {
        let routes = routes(&[]);
        for method in ["GET", "OPTIONS"] {
            let response = warp::test::request()
                .method(method)
                .path("/ok")
                .header("origin", "https://player.example")
                .header("access-control-request-method", "GET")
                .reply(&routes)
                .await;
            assert_eq!(response.status(), StatusCode::FORBIDDEN);
            assert!(!response
                .headers()
                .contains_key("access-control-allow-origin"));
        }

        let response = warp::test::request().path("/ok").reply(&routes).await;
        assert_eq!(response.status(), StatusCode::OK);
    }
}
