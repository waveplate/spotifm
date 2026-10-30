use rspotify::clients::BaseClient;
use rspotify::http::{BaseHttpClient, HttpClient, HttpError, Query};
use rspotify::{AuthCodePkceSpotify, ClientError, ClientResult, Config, Credentials, Token};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

const REFRESH_MARGIN_SECONDS: i64 = 300;

pub fn can_use_catalog_fallback(error: &ClientError) -> bool {
    match error {
        ClientError::InvalidToken | ClientError::ParseJson(_) => true,
        ClientError::Http(error) => match error.as_ref() {
            HttpError::StatusCode(response) => matches!(
                response.status().as_u16(),
                400 | 401 | 403 | 429 | 500..=599
            ),
            HttpError::Client(error) => error.is_connect() || error.is_timeout(),
        },
        _ => false,
    }
}

#[derive(Debug, Default)]
struct WebApiRequestHistory {
    total_gets: u64,
    recent_gets: VecDeque<Instant>,
}

impl WebApiRequestHistory {
    fn record_get(&mut self) -> (u64, usize) {
        let now = Instant::now();
        while self
            .recent_gets
            .front()
            .is_some_and(|started| now.duration_since(*started) >= Duration::from_secs(30))
        {
            self.recent_gets.pop_front();
        }
        self.total_gets = self.total_gets.saturating_add(1);
        self.recent_gets.push_back(now);
        (self.total_gets, self.recent_gets.len())
    }
}

pub fn retry_after_seconds(error: &ClientError) -> Option<u64> {
    let ClientError::Http(error) = error else {
        return None;
    };
    let HttpError::StatusCode(response) = error.as_ref() else {
        return None;
    };
    if response.status().as_u16() != 429 {
        return None;
    }
    response
        .headers()
        .get("retry-after")?
        .to_str()
        .ok()?
        .parse::<u64>()
        .ok()
}

fn needs_refresh(token: Option<&Token>) -> bool {
    token.is_none_or(|token| {
        token.expires_at.is_none_or(|expires_at| {
            expires_at <= chrono::Utc::now() + chrono::Duration::seconds(REFRESH_MARGIN_SECONDS)
        })
    })
}

/// All Web API requests, including paginated requests, use the same refresh
/// coordinator. Keep the raw PKCE client private so its built-in refresh cannot
/// discard the refresh token or race another request.
#[derive(Clone, Debug, Default)]
pub struct SpotifyClient {
    inner: AuthCodePkceSpotify,
    tokens: Arc<SpotifyTokenManager>,
    request_history: Arc<std::sync::Mutex<WebApiRequestHistory>>,
    pub catalog: Arc<crate::catalog::CatalogFallback>,
    catalog_fallback_until: Arc<std::sync::Mutex<Option<Instant>>>,
    catalog_failures: Arc<AtomicU64>,
}

impl SpotifyClient {
    pub fn new(mut inner: AuthCodePkceSpotify) -> Self {
        inner.config.token_refreshing = false;
        Self {
            inner,
            tokens: Default::default(),
            request_history: Default::default(),
            catalog: Default::default(),
            catalog_fallback_until: Default::default(),
            catalog_failures: Default::default(),
        }
    }

    pub fn with_session(mut self, session: librespot::core::Session) -> Self {
        self.catalog = Arc::new(crate::catalog::CatalogFallback::new(Some(session)));
        self
    }

    pub fn prefer_catalog_fallback(&self) -> bool {
        self.catalog_fallback_until
            .lock()
            .unwrap()
            .is_some_and(|until| Instant::now() < until)
    }

    pub fn catalog_failure_count(&self) -> u64 {
        self.catalog_failures.load(Ordering::Relaxed)
    }

    pub fn note_catalog_failure(&self, error: &ClientError) {
        if !can_use_catalog_fallback(error) {
            return;
        }
        self.catalog_failures.fetch_add(1, Ordering::Relaxed);
        let seconds = retry_after_seconds(error)
            .map(|seconds| seconds.saturating_add(1))
            .unwrap_or(60)
            .max(1);
        let now = Instant::now();
        let until = now
            .checked_add(Duration::from_secs(seconds))
            .unwrap_or(now + Duration::from_secs(3600));
        let mut current = self.catalog_fallback_until.lock().unwrap();
        *current = Some(current.map_or(until, |previous| previous.max(until)));
        eprintln!(
            "[Catalogue] Web API request failed; using catalogue backup for {seconds}s: {error}"
        );
    }

    pub async fn ensure_fresh(&self) -> ClientResult<()> {
        self.tokens.ensure_fresh(&self.inner).await
    }

    async fn fresh_token(&self) -> ClientResult<Token> {
        self.ensure_fresh().await?;
        self.inner
            .token
            .lock()
            .await
            .unwrap()
            .clone()
            .ok_or(ClientError::InvalidToken)
    }

    async fn api_get_web(&self, url: &str, payload: &Query<'_>) -> ClientResult<String> {
        let url = self.api_url(url);
        let token = self.fresh_token().await?;
        let result = self.get_with_token(&url, payload, &token).await;
        match result {
            Err(HttpError::StatusCode(response)) if response.status().as_u16() == 401 => {
                // The server can reject a token before its local expiry. Only
                // refresh if another request hasn't already replaced this token.
                let _guard = self.tokens.refresh_lock.lock().await;
                let token_is_unchanged = self
                    .inner
                    .token
                    .lock()
                    .await
                    .unwrap()
                    .as_ref()
                    .is_some_and(|current| current.access_token == token.access_token);
                if token_is_unchanged {
                    println!("[OAuth/Web API] Spotify rejected the access token; refreshing and retrying once.");
                    refresh_token_preserving_refresh_token(&self.inner).await?;
                }
                drop(_guard);
                let token = self.fresh_token().await?;
                Ok(self.get_with_token(&url, payload, &token).await?)
            }
            other => Ok(other?),
        }
    }

    async fn get_with_token(
        &self,
        url: &str,
        payload: &Query<'_>,
        token: &Token,
    ) -> Result<String, HttpError> {
        let (request_number, local_gets_30s) = self.request_history.lock().unwrap().record_get();
        let endpoint = url::Url::parse(url)
            .map(|mut endpoint| {
                endpoint.set_query(None);
                endpoint.set_fragment(None);
                endpoint.to_string()
            })
            .unwrap_or_else(|_| "unparseable URL".to_string());
        // Count actual outgoing requests, including retries. Exclude the query
        // text and credentials from diagnostics. This records traffic only;
        // the history never changes whether a request is sent.
        eprintln!(
            "[OAuth/Web API] Sending GET: endpoint={endpoint}, configured_client_id={}, request_number={request_number}, local_gets_30s={local_gets_30s}, search_type={:?}, search_limit={:?}",
            self.inner.creds.id,
            payload.get("type"),
            payload.get("limit"),
        );
        let mut result = self
            .get_http()
            .get(url, Some(&token.auth_headers()), payload)
            .await;
        if let Err(error) = &mut result {
            log_http_failure("request", &self.inner.creds.id, token, error).await;
        }
        result
    }
}

fn spotify_error_summary(body: &[u8], token: &Token) -> Option<String> {
    let json: serde_json::Value = serde_json::from_slice(body).ok()?;
    // Only record Spotify's error fields, never arbitrary response data.
    let mut fields = Vec::new();
    for pointer in [
        "/error/message",
        "/error/reason",
        "/error",
        "/error_description",
    ] {
        if let Some(value) = json.pointer(pointer).and_then(serde_json::Value::as_str) {
            let mut value = value.to_string();
            for secret in [
                Some(token.access_token.as_str()),
                token.refresh_token.as_deref(),
            ]
            .into_iter()
            .flatten()
            .filter(|secret| !secret.is_empty())
            {
                value = value.replace(secret, "[redacted]");
            }
            fields.push(value.chars().take(512).collect::<String>());
        }
    }
    (!fields.is_empty()).then(|| fields.join("; "))
}

async fn log_http_failure(phase: &str, client_id: &str, token: &Token, error: &mut HttpError) {
    let HttpError::StatusCode(response) = error else {
        return;
    };
    if !matches!(response.status().as_u16(), 400 | 401 | 403 | 429) {
        return;
    }
    let mut endpoint = response.url().clone();
    endpoint.set_query(None);
    endpoint.set_fragment(None);
    let retry_after = response
        .headers()
        .get("retry-after")
        .and_then(|value| value.to_str().ok());
    let expires_in_seconds = token
        .expires_at
        .map(|expiry| (expiry - chrono::Utc::now()).num_seconds());
    let has_refresh_token = token
        .refresh_token
        .as_deref()
        .is_some_and(|value| !value.is_empty());
    eprintln!(
        "[OAuth/Web API] {phase} failed: endpoint={endpoint}, status={}, client_id={client_id}, expires_in_seconds={expires_in_seconds:?}, has_refresh_token={has_refresh_token}, retry_after={retry_after:?}",
        response.status()
    );

    // Callers use the status and headers for retry/error handling. Consume only
    // a bounded error body so Spotify's explanation is no longer discarded.
    let body = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            if body.len() + chunk.len() > 8192 {
                break;
            }
            body.extend_from_slice(&chunk);
        }
        Ok::<_, reqwest::Error>(body)
    })
    .await;
    if let Ok(Ok(body)) = body {
        if let Some(summary) = spotify_error_summary(&body, token) {
            eprintln!("[OAuth/Web API] {phase} response from {endpoint}: {summary:?}");
        }
    }
}

#[async_trait::async_trait]
impl BaseClient for SpotifyClient {
    fn get_config(&self) -> &Config {
        self.inner.get_config()
    }

    fn get_http(&self) -> &HttpClient {
        self.inner.get_http()
    }

    fn get_creds(&self) -> &Credentials {
        self.inner.get_creds()
    }

    fn get_token(&self) -> Arc<rspotify::sync::Mutex<Option<Token>>> {
        self.inner.get_token()
    }

    async fn refetch_token(&self) -> ClientResult<Option<Token>> {
        self.refresh_token().await?;
        Ok(self.inner.token.lock().await.unwrap().clone())
    }

    async fn refresh_token(&self) -> ClientResult<()> {
        self.tokens.refresh(&self.inner).await
    }

    async fn auto_reauth(&self) -> ClientResult<()> {
        self.ensure_fresh().await
    }

    async fn api_get(&self, url: &str, payload: &Query<'_>) -> ClientResult<String> {
        let result = self.api_get_web(url, payload).await;
        if let Err(error) = &result {
            self.note_catalog_failure(error);
        }
        result
    }
}

/// Coordinates token refreshes for the shared rspotify client.
///
/// Spotify does not always return a new refresh token when an access token is
/// refreshed. rspotify's PKCE client replaces the complete token in that case,
/// which drops the existing refresh token and can clear the client token on the
/// next refresh. Keep refreshes serialized and retain the previous refresh
/// token when Spotify omits a replacement.
#[derive(Debug, Default)]
pub struct SpotifyTokenManager {
    refresh_lock: Mutex<()>,
}

impl SpotifyTokenManager {
    pub async fn refresh(&self, client: &AuthCodePkceSpotify) -> ClientResult<()> {
        let _guard = self.refresh_lock.lock().await;
        refresh_token_preserving_refresh_token(client).await
    }

    pub async fn ensure_fresh(&self, client: &AuthCodePkceSpotify) -> ClientResult<()> {
        let _guard = self.refresh_lock.lock().await;
        let should_refresh = needs_refresh(client.token.lock().await.unwrap().as_ref());

        if should_refresh {
            refresh_token_preserving_refresh_token(client).await
        } else {
            Ok(())
        }
    }
}

pub async fn refresh_token_preserving_refresh_token(
    client: &AuthCodePkceSpotify,
) -> ClientResult<()> {
    println!("[OAuth/Web API] Refreshing access token.");
    let previous_token = client
        .token
        .lock()
        .await
        .unwrap()
        .clone()
        .ok_or(ClientError::InvalidToken)?;
    let previous_refresh_token = previous_token
        .refresh_token
        .clone()
        .ok_or(ClientError::InvalidToken)?;

    let mut refreshed_token = match client.refetch_token().await {
        Ok(token) => token.ok_or(ClientError::InvalidToken)?,
        Err(mut error) => {
            if let ClientError::Http(http_error) = &mut error {
                log_http_failure("refresh", &client.creds.id, &previous_token, http_error).await;
            }
            return Err(error);
        }
    };

    if refreshed_token.refresh_token.is_none() {
        refreshed_token.refresh_token = Some(previous_refresh_token);
    }

    let expires_at = refreshed_token.expires_at;
    *client.token.lock().await.unwrap() = Some(refreshed_token);
    client.write_token_cache().await?;
    println!("[OAuth/Web API] Access token refreshed; expires at {expires_at:?}.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        needs_refresh, refresh_token_preserving_refresh_token, spotify_error_summary, SpotifyClient,
    };
    use chrono::{Duration, Utc};
    use rspotify::clients::BaseClient;
    use rspotify::{AuthCodePkceSpotify, Config, Credentials, OAuth, Token};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use warp::Filter;

    #[test]
    fn diagnostic_summary_records_only_error_fields_and_redacts_credentials() {
        let token = Token {
            access_token: "test-access-secret".into(),
            refresh_token: Some("test-refresh-secret".into()),
            ..Default::default()
        };
        let body = serde_json::json!({
            "error": {"message": "Rejected test-access-secret", "reason": "QUOTA_EXCEEDED"},
            "access_token": "unrelated-secret"
        });
        assert_eq!(
            spotify_error_summary(body.to_string().as_bytes(), &token).as_deref(),
            Some("Rejected [redacted]; QUOTA_EXCEEDED")
        );
        let body = serde_json::json!({"error": "invalid_grant", "error_description": "Invalid test-refresh-secret"});
        assert_eq!(
            spotify_error_summary(body.to_string().as_bytes(), &token).as_deref(),
            Some("invalid_grant; Invalid [redacted]")
        );
        assert_eq!(spotify_error_summary(b"not JSON", &token), None);
    }

    #[tokio::test]
    async fn rate_limit_preserves_status_retry_header_and_existing_token() {
        let refreshes = Arc::new(AtomicUsize::new(0));
        let counter = refreshes.clone();
        let token_route = warp::path!("api" / "token").and(warp::post()).map(move || {
            counter.fetch_add(1, Ordering::SeqCst);
            warp::reply()
        });
        let resource = warp::path("resource").map(|| {
            warp::reply::with_header(
                warp::reply::with_status(
                    warp::reply::json(
                        &serde_json::json!({"error":{"status":429,"message":"Too many requests"}}),
                    ),
                    warp::http::StatusCode::TOO_MANY_REQUESTS,
                ),
                "Retry-After",
                "17",
            )
        });
        let (address, server) =
            warp::serve(token_route.or(resource)).bind_ephemeral(([127, 0, 0, 1], 0));
        tokio::spawn(server);
        let client = mock_client(address).await;
        let before = client.inner.token.lock().await.unwrap().clone();
        let error = client
            .api_get("resource", &Default::default())
            .await
            .unwrap_err();
        let rspotify::ClientError::Http(error) = error else {
            panic!("expected HTTP error")
        };
        let rspotify::http::HttpError::StatusCode(response) = *error else {
            panic!("expected status error")
        };
        assert_eq!(response.status().as_u16(), 429);
        assert_eq!(response.headers()["retry-after"], "17");
        assert_eq!(refreshes.load(Ordering::SeqCst), 0);
        assert_eq!(*client.inner.token.lock().await.unwrap(), before);
    }

    #[test]
    fn refreshes_unknown_expiry_and_before_expiration() {
        let mut token = Token::default();
        assert!(needs_refresh(None));
        token.expires_at = None;
        assert!(needs_refresh(Some(&token)));
        token.expires_at = Some(Utc::now() + Duration::minutes(4));
        assert!(needs_refresh(Some(&token)));
        token.expires_at = Some(Utc::now() + Duration::hours(1));
        assert!(!needs_refresh(Some(&token)));
    }

    async fn mock_client(address: std::net::SocketAddr) -> SpotifyClient {
        let client = AuthCodePkceSpotify::with_config(
            Credentials::new_pkce("client-id"),
            OAuth::default(),
            Config {
                api_base_url: format!("http://{address}/"),
                auth_base_url: format!("http://{address}/"),
                token_cached: false,
                ..Default::default()
            },
        );
        *client.token.lock().await.unwrap() = Some(Token {
            access_token: "old-token".into(),
            refresh_token: Some("refresh-token".into()),
            expires_at: Some(Utc::now() + Duration::hours(1)),
            ..Default::default()
        });
        SpotifyClient::new(client)
    }

    #[tokio::test]
    async fn concurrent_unauthorized_requests_share_one_refresh_despite_future_expiry() {
        let refreshes = Arc::new(AtomicUsize::new(0));
        let counter = refreshes.clone();
        let token_route = warp::path!("api" / "token")
            .and(warp::post())
            .and(warp::body::form())
            .map(move |form: std::collections::HashMap<String, String>| {
                assert_eq!(form["refresh_token"], "refresh-token");
                counter.fetch_add(1, Ordering::SeqCst);
                warp::reply::json(&serde_json::json!({
                    "access_token": "new-token", "token_type": "Bearer", "expires_in": 3600
                }))
            });
        let barrier = Arc::new(tokio::sync::Barrier::new(2));
        let resource = warp::path("resource")
            .and(warp::header::<String>("authorization"))
            .and_then(move |authorization: String| {
                let barrier = barrier.clone();
                async move {
                    let status = if authorization == "Bearer old-token" {
                        barrier.wait().await;
                        warp::http::StatusCode::UNAUTHORIZED
                    } else {
                        assert_eq!(authorization, "Bearer new-token");
                        warp::http::StatusCode::OK
                    };
                    Ok::<_, std::convert::Infallible>(warp::reply::with_status("ok", status))
                }
            });
        let (address, server) =
            warp::serve(token_route.or(resource)).bind_ephemeral(([127, 0, 0, 1], 0));
        tokio::spawn(server);
        let client = mock_client(address).await;
        let cloned = client.clone();
        let query = Default::default();
        let (first, second) = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            tokio::join!(
                client.api_get("resource", &query),
                cloned.api_get("resource", &query)
            )
        })
        .await
        .expect("requests must not deadlock");
        assert_eq!(first.unwrap(), "ok");
        assert_eq!(second.unwrap(), "ok");
        assert_eq!(refreshes.load(Ordering::SeqCst), 1);
        assert_eq!(
            client
                .inner
                .token
                .lock()
                .await
                .unwrap()
                .as_ref()
                .unwrap()
                .refresh_token
                .as_deref(),
            Some("refresh-token")
        );
    }

    #[tokio::test]
    async fn retries_unauthorized_only_once_and_preserves_rotated_refresh_token() {
        let refreshes = Arc::new(AtomicUsize::new(0));
        let counter = refreshes.clone();
        let token_route = warp::path!("api" / "token").and(warp::post()).map(move || {
            counter.fetch_add(1, Ordering::SeqCst);
            warp::reply::json(&serde_json::json!({
                "access_token": "new-token", "token_type": "Bearer", "expires_in": 3600,
                "refresh_token": "rotated-refresh-token"
            }))
        });
        let requests = Arc::new(AtomicUsize::new(0));
        let counter = requests.clone();
        let resource = warp::path("resource").map(move || {
            counter.fetch_add(1, Ordering::SeqCst);
            warp::reply::with_status("rejected", warp::http::StatusCode::UNAUTHORIZED)
        });
        let (address, server) =
            warp::serve(token_route.or(resource)).bind_ephemeral(([127, 0, 0, 1], 0));
        tokio::spawn(server);
        let client = mock_client(address).await;
        assert!(client
            .api_get("resource", &Default::default())
            .await
            .is_err());
        assert_eq!(requests.load(Ordering::SeqCst), 2);
        assert_eq!(refreshes.load(Ordering::SeqCst), 1);
        assert_eq!(
            client
                .inner
                .token
                .lock()
                .await
                .unwrap()
                .as_ref()
                .unwrap()
                .refresh_token
                .as_deref(),
            Some("rotated-refresh-token")
        );
    }

    #[tokio::test]
    async fn network_refresh_failure_keeps_token_for_later_retry() {
        let token_route = warp::path!("api" / "token").and(warp::post()).map(|| {
            warp::reply::with_status("unavailable", warp::http::StatusCode::SERVICE_UNAVAILABLE)
        });
        let (address, server) = warp::serve(token_route).bind_ephemeral(([127, 0, 0, 1], 0));
        tokio::spawn(server);
        let client = mock_client(address).await;
        client
            .inner
            .token
            .lock()
            .await
            .unwrap()
            .as_mut()
            .unwrap()
            .expires_at = None;
        let before = client.inner.token.lock().await.unwrap().clone();
        assert!(client.ensure_fresh().await.is_err());
        assert_eq!(*client.inner.token.lock().await.unwrap(), before);
    }

    #[tokio::test]
    async fn refresh_retains_existing_refresh_token_when_response_omits_it() {
        let token_route = warp::path!("api" / "token").and(warp::post()).map(|| {
            warp::reply::json(&serde_json::json!({
                "access_token": "new-access-token",
                "token_type": "Bearer",
                "expires_in": 3600,
                "scope": "streaming"
            }))
        });
        let (address, server) = warp::serve(token_route).bind_ephemeral(([127, 0, 0, 1], 0));
        tokio::spawn(server);

        let config = Config {
            auth_base_url: format!("http://{address}/"),
            token_cached: false,
            ..Default::default()
        };
        let client = AuthCodePkceSpotify::with_config(
            Credentials::new_pkce("client-id"),
            OAuth::default(),
            config,
        );
        *client.token.lock().await.unwrap() = Some(Token {
            access_token: "old-access-token".to_string(),
            expires_in: Duration::seconds(3600),
            expires_at: Some(Utc::now()),
            refresh_token: Some("existing-refresh-token".to_string()),
            scopes: Default::default(),
        });

        refresh_token_preserving_refresh_token(&client)
            .await
            .unwrap();

        let token = client.token.lock().await.unwrap().clone().unwrap();
        assert_eq!(token.access_token, "new-access-token");
        assert_eq!(
            token.refresh_token.as_deref(),
            Some("existing-refresh-token")
        );
    }

    #[tokio::test]
    async fn failed_refresh_does_not_clear_the_current_token() {
        let client = AuthCodePkceSpotify::default();
        let current_token = Token {
            access_token: "current-access-token".to_string(),
            refresh_token: None,
            ..Default::default()
        };
        *client.token.lock().await.unwrap() = Some(current_token.clone());

        assert!(refresh_token_preserving_refresh_token(&client)
            .await
            .is_err());
        assert_eq!(
            client.token.lock().await.unwrap().as_ref(),
            Some(&current_token)
        );
    }
}
