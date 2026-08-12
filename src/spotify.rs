use rspotify::clients::BaseClient;
use rspotify::{AuthCodePkceSpotify, ClientError, ClientResult};
use tokio::sync::Mutex;

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
        let should_refresh = client
            .token
            .lock()
            .await
            .unwrap()
            .as_ref()
            .is_none_or(rspotify::Token::is_expired);

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
    let previous_refresh_token = client
        .token
        .lock()
        .await
        .unwrap()
        .as_ref()
        .and_then(|token| token.refresh_token.clone())
        .ok_or(ClientError::InvalidToken)?;

    let mut refreshed_token = client
        .refetch_token()
        .await?
        .ok_or(ClientError::InvalidToken)?;

    if refreshed_token.refresh_token.is_none() {
        refreshed_token.refresh_token = Some(previous_refresh_token);
    }

    *client.token.lock().await.unwrap() = Some(refreshed_token);
    client.write_token_cache().await
}

#[cfg(test)]
mod tests {
    use super::refresh_token_preserving_refresh_token;
    use chrono::{Duration, Utc};
    use rspotify::{AuthCodePkceSpotify, Config, Credentials, OAuth, Token};
    use warp::Filter;

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
