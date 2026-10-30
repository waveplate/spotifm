//! Anonymous web-player search. No account credentials are read or persisted.
//! Protocol reference: https://github.com/sjdonado/idonthavespotify/blob/main/src/adapters/spotify.ts

use hmac::{Hmac, Mac};
use regex::Regex;
use reqwest::{Client, Response, StatusCode};
use serde_json::{json, Value};
use sha1::Sha1;
use std::collections::HashMap;
use std::sync::LazyLock;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

const PLAYER: &str = "https://open.spotify.com";
const SEARCH: &str = "https://api-partner.spotify.com/pathfinder/v1/query";
const SEARCH_HASH: &str = "75bbf6bfcfdf85b8fc828417bfad92b7cd66bf7f556d85670f4da8292373ebec";
const CACHE_TTL: Duration = Duration::from_secs(60);
static PLAYER_BUNDLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"https://[^\s"'<>]+/(?:mobile-)?web-player\.[0-9a-f]+\.js"#).unwrap()
});
static TOKEN_CONFIG: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"\{\s*secret\s*:\s*["']([^"']+)["']\s*,\s*version\s*:\s*(\d+)\s*\}"#).unwrap()
});

#[derive(Clone, Debug)]
pub struct FallbackError {
    pub message: String,
    pub status: StatusCode,
    pub retry_after: Option<u64>,
}

impl FallbackError {
    pub fn protocol(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            status: StatusCode::BAD_GATEWAY,
            retry_after: None,
        }
    }

    fn transport(error: reqwest::Error) -> Self {
        // Bootstrap URLs contain a TOTP. Never include a request URL or response
        // body in errors; neither is needed to explain a network failure.
        Self::protocol(format!("Spotify anonymous search: {}", error.without_url()))
    }
}

impl std::fmt::Display for FallbackError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

struct VisitorToken {
    access_token: String,
    expires_at: Instant,
}

#[derive(Default)]
struct VisitorState {
    token: Option<VisitorToken>,
    searches: HashMap<(String, u32), (Instant, Value)>,
}

pub struct AnonymousSearch {
    http: Client,
    // Serialize bootstrap and searches so concurrent requests share one visitor
    // token and identical queries reuse a response rather than stampeding.
    state: Mutex<VisitorState>,
}

impl std::fmt::Debug for AnonymousSearch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnonymousSearch").finish_non_exhaustive()
    }
}

impl Default for AnonymousSearch {
    fn default() -> Self {
        Self {
            http: Client::builder()
                .user_agent(concat!(
                    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 ",
                    "(KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36"
                ))
                .cookie_store(true)
                .connect_timeout(Duration::from_secs(10))
                .timeout(Duration::from_secs(25))
                .build()
                .expect("Anonymous Spotify HTTP client configuration is valid"),
            state: Mutex::new(VisitorState::default()),
        }
    }
}

async fn checked(response: Response) -> Result<Response, FallbackError> {
    if response.status().is_success() {
        return Ok(response);
    }
    let status = response.status();
    let retry_after = response
        .headers()
        .get("retry-after")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse().ok());
    Err(FallbackError {
        message: format!("Spotify anonymous search returned {status}"),
        status,
        retry_after,
    })
}

impl AnonymousSearch {
    async fn get_text(&self, url: &str) -> Result<String, FallbackError> {
        checked(
            self.http
                .get(url)
                .send()
                .await
                .map_err(FallbackError::transport)?,
        )
        .await?
        .text()
        .await
        .map_err(FallbackError::transport)
    }

    async fn bootstrap(&self) -> Result<VisitorToken, FallbackError> {
        let started = Instant::now();
        let html = self.get_text(&format!("{PLAYER}/")).await?;
        let bundle = PLAYER_BUNDLE.find(&html).ok_or_else(|| {
            FallbackError::protocol("Spotify anonymous search could not find the web-player bundle")
        })?;
        let javascript = self.get_text(bundle.as_str()).await?;
        let (secret, version) = TOKEN_CONFIG
            .captures_iter(&javascript)
            .filter_map(|config| Some((config[1].to_string(), config[2].parse::<u32>().ok()?)))
            .max_by_key(|(_, version)| *version)
            .ok_or_else(|| {
                FallbackError::protocol("Spotify visitor-token configuration changed")
            })?;

        // Fetch the server clock after the larger bundle download. Token expiry
        // uses that clock plus a monotonic timer, independent of local UTC skew.
        let clock_started = Instant::now();
        let clock: Value = checked(
            self.http
                .get(format!("{PLAYER}/api/server-time"))
                .send()
                .await
                .map_err(FallbackError::transport)?,
        )
        .await?
        .json()
        .await
        .map_err(FallbackError::transport)?;
        let server_time = clock["serverTime"]
            .as_u64()
            .ok_or_else(|| FallbackError::protocol("Spotify did not provide its server time"))?;
        let key: String = secret
            .encode_utf16()
            .enumerate()
            .map(|(index, code)| (code ^ ((index % 33) as u16 + 9)).to_string())
            .collect();
        let mut mac = Hmac::<Sha1>::new_from_slice(key.as_bytes())
            .map_err(|_| FallbackError::protocol("Invalid Spotify visitor-token configuration"))?;
        mac.update(&(server_time / 30).to_be_bytes());
        let digest = mac.finalize().into_bytes();
        let offset = (digest[digest.len() - 1] & 15) as usize;
        let code = (u32::from_be_bytes(digest[offset..offset + 4].try_into().unwrap())
            & 0x7fff_ffff)
            % 1_000_000;
        let token: Value = checked(
            self.http
                .get(format!("{PLAYER}/api/token"))
                .query(&[
                    ("reason", "init".to_string()),
                    ("productType", "web-player".to_string()),
                    ("totp", format!("{code:06}")),
                    ("totpVer", version.to_string()),
                    ("ts", server_time.to_string()),
                ])
                .header("Origin", PLAYER)
                .header("Referer", format!("{PLAYER}/"))
                .header("Accept", "application/json")
                .send()
                .await
                .map_err(FallbackError::transport)?,
        )
        .await?
        .json()
        .await
        .map_err(FallbackError::transport)?;
        if token["isAnonymous"].as_bool() != Some(true) {
            return Err(FallbackError::protocol(
                "Spotify did not issue an anonymous visitor token",
            ));
        }
        let access_token = token["accessToken"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                FallbackError::protocol("Spotify did not issue a visitor access token")
            })?;
        let lifetime = token["accessTokenExpirationTimestampMs"]
            .as_u64()
            .and_then(|expiry| expiry.checked_sub(server_time.saturating_mul(1000)))
            .map(Duration::from_millis)
            .and_then(|lifetime| {
                lifetime.checked_sub(clock_started.elapsed() + Duration::from_secs(30))
            })
            .filter(|lifetime| *lifetime < Duration::from_secs(24 * 3600))
            .ok_or_else(|| {
                FallbackError::protocol("Spotify returned an invalid visitor-token expiry")
            })?;
        println!(
            "[Catalogue/Anonymous] Visitor token renewed in {:.1}s.",
            started.elapsed().as_secs_f64()
        );
        Ok(VisitorToken {
            access_token: access_token.to_owned(),
            expires_at: Instant::now() + lifetime,
        })
    }

    pub async fn search(&self, query: &str, limit: u32) -> Result<Value, FallbackError> {
        let mut state = self.state.lock().await;
        let limit = limit.clamp(1, 50);
        let key = (query.to_owned(), limit);
        state.searches.retain(|_, (at, _)| at.elapsed() < CACHE_TTL);
        if let Some((_, result)) = state.searches.get(&key) {
            return Ok(result.clone());
        }
        let variables =
            json!({ "searchTerm": query, "offset": 0, "limit": limit, "numberOfTopResults": 5 });
        let extensions = json!({ "persistedQuery": { "version": 1, "sha256Hash": SEARCH_HASH } });
        for attempt in 0..2 {
            if state
                .token
                .as_ref()
                .is_none_or(|token| Instant::now() >= token.expires_at)
            {
                state.token = Some(self.bootstrap().await?);
            }
            let token = &state.token.as_ref().unwrap().access_token;
            let response = self
                .http
                .get(SEARCH)
                .query(&[
                    ("operationName", "searchDesktop"),
                    ("variables", &variables.to_string()),
                    ("extensions", &extensions.to_string()),
                ])
                .bearer_auth(token)
                .header("app-platform", "WebPlayer")
                .header("Origin", PLAYER)
                .header("Referer", format!("{PLAYER}/"))
                .send()
                .await
                .map_err(FallbackError::transport)?;
            if response.status() == StatusCode::UNAUTHORIZED && attempt == 0 {
                state.token = None;
                continue;
            }
            let result: Value = checked(response)
                .await?
                .json()
                .await
                .map_err(FallbackError::transport)?;
            let search = result.pointer("/data/searchV2").filter(|value| value.is_object())
                .or_else(|| result.pointer("/data/search").filter(|value| value.is_object()))
                .ok_or_else(|| FallbackError::protocol("Spotify anonymous search returned no search data; the web-player protocol may have changed"))?;
            // GraphQL may include partial data alongside errors. Missing category
            // sections are rejected by the normalizer, never treated as empty.
            let search = search.clone();
            if state.searches.len() >= 128 {
                state.searches.clear();
            }
            state.searches.insert(key, (Instant::now(), search.clone()));
            return Ok(search);
        }
        unreachable!("anonymous search retries a rejected visitor token only once")
    }
}
