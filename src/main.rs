#![recursion_limit = "1024"]

macro_rules! println {
    ($($arg:tt)*) => {{
        let message = std::format!($($arg)*);
        std::println!("{message}");
        crate::logging::write_line("INFO", &message);
    }};
}

macro_rules! eprintln {
    ($($arg:tt)*) => {{
        let message = std::format!($($arg)*);
        std::eprintln!("{message}");
        crate::logging::write_line("ERROR", &message);
    }};
}

mod audio;
mod cache_paths;
mod cli;
mod handlers;
mod logging;
mod models;
mod spotify;
mod state;
mod tls;

use librespot::core::authentication::Credentials as LibrespotCredentials;
use librespot::core::config::SessionConfig;
use librespot::core::session::Session;
use librespot::playback::config::PlayerConfig;
use rspotify::{prelude::*, AuthCodePkceSpotify, Config as RConfig, Credentials as RCreds, OAuth};
use rustls::crypto::ring::default_provider;
use std::{
    error::Error,
    net::{SocketAddr, ToSocketAddrs},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio::sync::mpsc::unbounded_channel;
use url::Url;
use warp::Filter;

use crate::audio::playback::playback_manager;
use crate::audio::stream_manager::{StreamManager, StreamManagerConfig};
use crate::cli::{join_endpoint_path, normalize_endpoint_path, Cli};
use crate::handlers::{
    authorization_transition_page, handle_lyrics_rest, handle_lyrics_ws, handle_minimal_player,
    handle_next, handle_np, handle_oauth_page, handle_oauth_status, handle_play_category,
    handle_play_category_search, handle_player, handle_player_asset_name,
    handle_player_audio_worklet, handle_player_service_worker,
    handle_player_vendor_ogg_vorbis_decoder, handle_playlist, handle_playlist_add,
    handle_playlist_add_json, handle_playlist_delete, handle_playlist_delete_where,
    handle_playlist_delete_where_json, handle_playlist_get, handle_playlist_play_track,
    handle_playlist_remove_track, handle_playlist_shuffle, handle_playlist_shuffle_named,
    handle_playlist_sort, handle_playlist_sort_named, handle_playlist_switch, handle_playlists,
    handle_queue_category, handle_queue_category_search, handle_queue_list,
    handle_remove_queue_track, handle_search_category, handle_skip, handle_skip_category,
    handle_skip_n, handle_stream,
};
use crate::state::AppState;
use crate::tls::{resolve_tls_files, TlsFiles};

const NCSPOT_WEB_API_CLIENT_ID: &str = "d420a117a32841c2b3474932e49fb54b";
const WEB_API_SCOPES: &[&str] = &[
    "playlist-read-private",
    "playlist-read-collaborative",
    "user-read-private",
];

#[derive(serde::Deserialize)]
struct OAuthCallback {
    code: String,
    state: Option<String>,
}

fn effective_web_api_redirect_uri(cli: &Cli) -> String {
    if cli.client_id == NCSPOT_WEB_API_CLIENT_ID {
        if let Ok(url) = Url::parse(&cli.redirect_uri) {
            let is_loopback = matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "::1"));
            if url.scheme() == "http" && is_loopback {
                return format!("http://127.0.0.1:{}/login", cli.api_port);
            }
        }
    }

    cli.redirect_uri.clone()
}

fn oauth_gateway_routes(
    auth_url: String,
    callback_path: String,
    expected_state: String,
    code_tx: tokio::sync::mpsc::Sender<String>,
    phase: String,
) -> impl Filter<Extract = (warp::http::Response<String>,), Error = std::convert::Infallible> + Clone
{
    let callback_received = Arc::new(AtomicBool::new(false));
    let status_callback_received = callback_received.clone();
    let status_phase = phase.clone();
    let status_route = warp::path!("oauth" / "status")
        .and(warp::get())
        .map(move || {
            let status = if status_callback_received.load(Ordering::Acquire) {
                "checking"
            } else {
                "authorization_required"
            };
            warp::http::Response::builder()
                .status(warp::http::StatusCode::OK)
                .header("content-type", "application/json; charset=utf-8")
                .header("cache-control", "no-store")
                .body(
                    serde_json::json!({
                        "status": status,
                        "phase": status_phase,
                    })
                    .to_string(),
                )
                .unwrap()
        });

    let auth_url_for_route = auth_url.clone();
    let oauth_route = warp::path("oauth").and(warp::path::end()).map(move || {
        warp::http::Response::builder()
            .status(warp::http::StatusCode::FOUND)
            .header("location", auth_url_for_route.clone())
            .body(String::new())
            .unwrap()
    });

    let callback_received_for_route = callback_received;
    let callback_route = endpoint_path_filter(callback_path)
        .and(warp::get())
        .and(warp::query::<OAuthCallback>())
        .map(move |callback: OAuthCallback| {
            if callback.state.as_deref() != Some(expected_state.as_str()) {
                return warp::http::Response::builder()
                    .status(warp::http::StatusCode::BAD_REQUEST)
                    .header("content-type", "text/plain; charset=utf-8")
                    .body("Invalid OAuth state".to_string())
                    .unwrap();
            }

            callback_received_for_route.store(true, Ordering::Release);
            let _ = code_tx.try_send(callback.code);
            let body = authorization_transition_page(&phase);
            warp::http::Response::builder()
                .status(warp::http::StatusCode::OK)
                .header("content-type", "text/html; charset=utf-8")
                .header("cache-control", "no-store")
                .body(body)
                .unwrap()
        });

    let pre_auth_redirect = warp::any().map(|| {
        warp::http::Response::builder()
            .status(warp::http::StatusCode::FOUND)
            .header("location", "/oauth")
            .body(String::new())
            .unwrap()
    });

    status_route
        .or(oauth_route)
        .unify()
        .or(callback_route)
        .unify()
        .or(pre_auth_redirect)
        .unify()
}

async fn authorize_with_gateway(
    spotify: &mut AuthCodePkceSpotify,
    cli: &Cli,
    tls_files: &TlsFiles,
    phase: &str,
) -> Result<(), Box<dyn Error>> {
    let auth_url = spotify.get_authorize_url(None)?;
    let expected_state = spotify.oauth.state.clone();
    let redirect_url = Url::parse(&spotify.oauth.redirect_uri)?;
    let callback_path = redirect_url.path().to_string();
    let (code_tx, mut code_rx) = tokio::sync::mpsc::channel::<String>(1);
    let routes = oauth_gateway_routes(
        auth_url.clone(),
        callback_path,
        expected_state,
        code_tx,
        phase.to_string(),
    );

    let listen_ip = cli
        .oauth_ip
        .clone()
        .unwrap_or_else(|| "0.0.0.0".to_string());
    let redirect_port = redirect_url.port_or_known_default().ok_or_else(|| {
        format!(
            "OAuth redirect URI has no usable port: {}",
            spotify.oauth.redirect_uri
        )
    })?;
    let listen_port = cli.oauth_port.unwrap_or(redirect_port);
    let callback_uses_tls = match redirect_url.scheme() {
        "http" => false,
        "https" => true,
        scheme => return Err(format!("Unsupported OAuth redirect URI scheme: {scheme}").into()),
    };
    let callback_uses_gateway = if callback_uses_tls {
        redirect_port == cli.tls_port || listen_port == cli.tls_port
    } else {
        redirect_port == cli.api_port || listen_port == cli.api_port
    };
    let callback_listener_addr = if callback_uses_gateway {
        None
    } else {
        Some(resolve_oauth_listener_addr(&listen_ip, listen_port)?)
    };

    let api_addr = format!("{}:{}", cli.api_ip, cli.api_port).parse::<SocketAddr>()?;
    let tls_addr = format!("{}:{}", cli.api_ip, cli.tls_port).parse::<SocketAddr>()?;
    let (http_shutdown_tx, http_shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let (tls_shutdown_tx, tls_shutdown_rx) = tokio::sync::oneshot::channel::<()>();

    let (_, http_server) =
        warp::serve(routes.clone()).bind_with_graceful_shutdown(api_addr, async move {
            let _ = http_shutdown_rx.await;
        });
    tokio::spawn(http_server);
    let (_, tls_server) = warp::serve(routes.clone())
        .tls()
        .cert_path(&tls_files.cert)
        .key_path(&tls_files.key)
        .bind_with_graceful_shutdown(tls_addr, async move {
            let _ = tls_shutdown_rx.await;
        });
    tokio::spawn(tls_server);

    let mut callback_shutdown_tx = None;
    if let Some(listen_addr) = callback_listener_addr {
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        if callback_uses_tls {
            let (_, callback_server) = warp::serve(routes)
                .tls()
                .cert_path(&tls_files.cert)
                .key_path(&tls_files.key)
                .bind_with_graceful_shutdown(listen_addr, async move {
                    let _ = shutdown_rx.await;
                });
            tokio::spawn(callback_server);
        } else {
            let (_, callback_server) =
                warp::serve(routes).bind_with_graceful_shutdown(listen_addr, async move {
                    let _ = shutdown_rx.await;
                });
            tokio::spawn(callback_server);
        }
        callback_shutdown_tx = Some(shutdown_tx);
        println!("[OAuth/{phase}] Callback listener available at {listen_addr}.");
    }

    let local_oauth_url = format!("http://127.0.0.1:{}/oauth", cli.api_port);
    println!("[OAuth/{phase}] Authorization gateway is ready:");
    println!("        {local_oauth_url}");
    println!("        https://127.0.0.1:{}/oauth", cli.tls_port);
    println!("[OAuth/{phase}] Direct Spotify authorization URL:");
    println!("        {auth_url}");
    println!("[OAuth/{phase}] All other routes redirect to /oauth until this step completes.");
    println!(
        "[OAuth/{phase}] Spotify callback: {}",
        spotify.oauth.redirect_uri
    );

    if let Some(ref url_file) = cli.url_file {
        std::fs::write(url_file, format!("{local_oauth_url}\n"))?;
        println!(
            "[OAuth/{phase}] Gateway URL written to {}",
            url_file.display()
        );
    }

    if !cli.no_browser {
        open_browser(&local_oauth_url);
    }

    let code_result = tokio::select! {
        result = code_rx.recv() => result.ok_or_else(|| "OAuth callback channel closed".to_string()),
        _ = tokio::time::sleep(Duration::from_secs(300)) => {
            Err(format!("{phase} OAuth authorization timed out after 5 minutes"))
        }
    };

    let token_result = match code_result {
        Ok(code) => spotify
            .request_token(&code)
            .await
            .map_err(|error| -> Box<dyn Error> { Box::new(error) }),
        Err(error) => Err(error.into()),
    };

    let _ = http_shutdown_tx.send(());
    let _ = tls_shutdown_tx.send(());
    if let Some(shutdown_tx) = callback_shutdown_tx {
        let _ = shutdown_tx.send(());
    }
    tokio::time::sleep(Duration::from_millis(150)).await;
    token_result?;
    println!("[OAuth/{phase}] Token successfully exchanged.");
    Ok(())
}

fn try_import_ncspot_token(dest_path: &std::path::Path) {
    if dest_path.exists() {
        return;
    }
    let mut src_path = None;
    #[cfg(target_os = "linux")]
    {
        if let Ok(home) = std::env::var("HOME") {
            let path = std::path::PathBuf::from(home).join(".cache/ncspot/rspotify_token.json");
            if path.exists() {
                src_path = Some(path);
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        if let Ok(home) = std::env::var("HOME") {
            let path =
                std::path::PathBuf::from(home).join("Library/Caches/ncspot/rspotify_token.json");
            if path.exists() {
                src_path = Some(path);
            }
        }
    }
    #[cfg(target_os = "windows")]
    {
        if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
            let path =
                std::path::PathBuf::from(local_app_data).join("ncspot/cache/rspotify_token.json");
            if path.exists() {
                src_path = Some(path);
            }
        }
    }
    if let Some(src) = src_path {
        println!(
            "[OAuth] 💡 Found existing ncspot token at {}. Importing for a seamless first run...",
            src.display()
        );
        if let Err(e) = std::fs::copy(&src, dest_path) {
            eprintln!("[OAuth] ⚠️ Failed to copy ncspot token: {}", e);
        }
    }
}

async fn request_librespot_credentials(
    cli: &Cli,
    session_config: &SessionConfig,
    tls_files: &TlsFiles,
) -> Result<LibrespotCredentials, Box<dyn Error>> {
    println!(
        "[OAuth/Playback] No reusable playback credential was found; starting the dedicated librespot authorization."
    );
    println!(
        "[OAuth/Playback] This is separate from the Web API authorization and happens only when playback credentials are missing or rejected."
    );

    let redirect_uri = format!("http://127.0.0.1:{}/login", cli.api_port);
    let credentials = RCreds::new_pkce(&session_config.client_id);
    let oauth = OAuth {
        redirect_uri,
        scopes: ["streaming".to_string()].into_iter().collect(),
        ..Default::default()
    };
    let config = RConfig {
        token_cached: false,
        token_refreshing: false,
        ..Default::default()
    };
    let mut spotify = AuthCodePkceSpotify::with_config(credentials, oauth, config);
    authorize_with_gateway(&mut spotify, cli, tls_files, "Playback").await?;

    let access_token = spotify
        .token
        .lock()
        .await
        .unwrap()
        .as_ref()
        .map(|token| token.access_token.clone())
        .ok_or_else(|| std::io::Error::other("playback OAuth returned no access token"))?;
    Ok(LibrespotCredentials::with_access_token(access_token))
}

async fn connect_librespot_session(
    cli: &Cli,
    cache_dir: &std::path::Path,
    tls_files: &TlsFiles,
) -> Result<Session, Box<dyn Error>> {
    let session_config = SessionConfig::default();
    let cache = librespot::core::cache::Cache::new(
        Some(cache_dir.to_path_buf()),
        None::<std::path::PathBuf>,
        None::<std::path::PathBuf>,
        None,
    )?;
    let cached_credentials = cache.credentials();
    let using_cached_credentials = cached_credentials.is_some();
    let credentials = match cached_credentials {
        Some(credentials) => {
            println!("[OAuth/Playback] Reusing cached librespot credentials.");
            credentials
        }
        None => request_librespot_credentials(cli, &session_config, tls_files).await?,
    };

    let session = Session::new(session_config.clone(), Some(cache));
    match Session::connect(&session, credentials, true).await {
        Ok(()) => {
            println!("-> Librespot playback session connected.");
            Ok(session)
        }
        Err(error) if using_cached_credentials => {
            eprintln!(
                "[OAuth/Playback] Cached playback credentials were rejected ({error}); requesting a fresh playback authorization."
            );
            let credentials =
                request_librespot_credentials(cli, &session_config, tls_files).await?;
            let cache = librespot::core::cache::Cache::new(
                Some(cache_dir.to_path_buf()),
                None::<std::path::PathBuf>,
                None::<std::path::PathBuf>,
                None,
            )?;
            let session = Session::new(session_config, Some(cache));
            Session::connect(&session, credentials, true).await?;
            println!("-> Librespot playback session connected with fresh credentials.");
            Ok(session)
        }
        Err(error) => Err(error.into()),
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // 1) Parse CLI and merge with TOML config
    let cli = Cli::resolve();
    if cli.api_port == cli.tls_port {
        return Err(format!(
            "HTTP api_port and HTTPS tls_port must be different (both are {})",
            cli.api_port
        )
        .into());
    }
    let cache_dir = cache_paths::spotifm_cache_dir(cli.playlist_path.as_deref());
    std::fs::create_dir_all(&cache_dir)?;
    let log_path = cache_dir.join("spotifm.log");
    logging::init(&log_path)?;
    println!("-> File logging enabled at {}", log_path.display());
    println!(
        "-> Starting Spotifm on {} (HTTP {}, HTTPS {}; player: {}, stream: {}, API keys: {})",
        cli.api_ip,
        cli.api_port,
        cli.tls_port,
        cli.player_endpoint,
        cli.stream_endpoint,
        cli.api_keys
            .as_ref()
            .map_or(0, std::collections::HashMap::len)
    );

    // 2) TLS setup for rspotify
    default_provider()
        .install_default()
        .expect("rustls setup failed");
    println!("-> Rustls TLS provider installed.");

    // 3) OAuth & librespot session
    let redirect_uri = effective_web_api_redirect_uri(&cli);
    if redirect_uri != cli.redirect_uri {
        println!(
            "[OAuth/Web API] Using the configured HTTP server for the loopback callback: {redirect_uri}"
        );
    }

    let tls_files = resolve_tls_files(
        cli.tls_cert.as_deref(),
        cli.tls_key.as_deref(),
        &cache_dir,
        &cli.api_ip,
    )?;
    println!(
        "-> HTTPS certificate: {}{}",
        tls_files.cert.display(),
        if tls_files.generated {
            " (new self-signed certificate)"
        } else {
            ""
        }
    );

    if !cli.skip_import {
        if let Some(cache_root) = cache_paths::cache_home() {
            match cache_paths::import_librespot_credentials(&cache_root, &cache_dir) {
                Ok(Some(source)) => println!(
                    "[OAuth] Found reusable librespot credentials at {} and imported them.",
                    source.display()
                ),
                Ok(None) => {}
                Err(error) => {
                    eprintln!("[OAuth] ⚠️ Failed to import reusable librespot credentials: {error}")
                }
            }
        }
    }

    // Playback and Web API authentication are intentionally separate. A successful
    // librespot login stores a reusable credential blob in credentials.json.
    let session = connect_librespot_session(&cli, &cache_dir, &tls_files).await?;

    // 4) rspotify Web API client setup with local token caching
    let token_cache_path = cache_dir.join("rspotify_token.json");
    if cli.client_id == NCSPOT_WEB_API_CLIENT_ID && !cli.skip_import {
        try_import_ncspot_token(&token_cache_path);
    }
    let rconfig = RConfig {
        cache_path: token_cache_path.clone(),
        token_cached: true,
        token_refreshing: true,
        ..Default::default()
    };
    let rcreds = RCreds::new_pkce(&cli.client_id);
    let ropts = OAuth {
        redirect_uri: redirect_uri.clone(),
        scopes: WEB_API_SCOPES
            .iter()
            .map(|scope| (*scope).to_string())
            .collect(),
        ..Default::default()
    };
    let mut spotify = AuthCodePkceSpotify::with_config(rcreds, ropts, rconfig);

    // Check if we have a valid cached token
    let mut has_token = false;
    if let Ok(Some(tok)) = spotify.read_token_cache(true).await {
        // ncspot's token mapper leaves this field empty, so an empty set means
        // "unknown" rather than "no scopes" for imported tokens.
        let cached_scopes_are_sufficient = tok.scopes.is_empty()
            || WEB_API_SCOPES
                .iter()
                .all(|scope| tok.scopes.contains(*scope));
        let token_is_expired = tok.is_expired();
        if !cached_scopes_are_sufficient {
            let missing_scopes = WEB_API_SCOPES
                .iter()
                .filter(|scope| !tok.scopes.contains(**scope))
                .copied()
                .collect::<Vec<_>>()
                .join(", ");
            println!(
                "[OAuth/Web API] Cached token is missing required scope(s): {missing_scopes}. Reauthorization is required."
            );
        } else if token_is_expired {
            *spotify.token.lock().await.unwrap() = Some(tok);
            if let Err(e) = spotify::refresh_token_preserving_refresh_token(&spotify).await {
                eprintln!("[OAuth/Web API] ⚠️ Failed to refresh expired cached token: {e}");
            } else {
                has_token = true;
                println!("[OAuth/Web API] Refreshed expired cached token.");
            }
        } else {
            *spotify.token.lock().await.unwrap() = Some(tok);
            has_token = true;
            println!("[OAuth/Web API] Loaded cached token; it is still valid.");
        }
    }

    if !has_token {
        authorize_with_gateway(&mut spotify, &cli, &tls_files, "Web API").await?;
        println!("[OAuth/Web API] Token successfully cached.");
    }

    let bitrate = match cli.bitrate {
        96 => librespot::playback::config::Bitrate::Bitrate96,
        160 => librespot::playback::config::Bitrate::Bitrate160,
        320 => librespot::playback::config::Bitrate::Bitrate320,
        _ => {
            eprintln!(
                "[Config] Warning: Invalid Spotify bitrate {} kbps. Defaulting to 160 kbps.",
                cli.bitrate
            );
            librespot::playback::config::Bitrate::Bitrate160
        }
    };

    let player_cfg = PlayerConfig {
        bitrate,
        position_update_interval: Some(Duration::from_secs(1)),
        passthrough: cli.passthrough,
        ..Default::default()
    };

    let total_samples = Arc::new(Mutex::new(0u64));

    // 4.5) GStreamer Stream Manager setup
    let stream_mgr = StreamManager::new(
        StreamManagerConfig {
            pipeline_name: cli.pipeline.clone(),
            bitrate: cli.bitrate,
            queue_size: cli.queue_size,
            max_buffers: cli.max_buffers,
            silence_interval_ms: cli.silence_interval,
            custom_pipeline: cli.gst_pipeline.clone(),
            passthrough: player_cfg.passthrough,
        },
        total_samples.clone(),
    )
    .expect("Failed to initialize GStreamer StreamManager");
    let tx_pcm = stream_mgr.tx_pcm;
    let tx_mp3 = stream_mgr.tx_mp3;
    let stream_headers = stream_mgr.stream_headers.clone();
    let playback_timeline = stream_mgr.playback_timeline.clone();
    let ogg_page_index = stream_mgr.ogg_page_index.clone();

    // 4.6) Lyrics WebSocket Broadcaster & Cache
    let (tx_lyrics_ws, _) = tokio::sync::broadcast::channel(cli.capacity);
    let current_lyrics = Arc::new(Mutex::new(None));

    // 5) Command channel & shared state
    let (cmd_tx, cmd_rx) = unbounded_channel();
    let state = Arc::new(AppState {
        session: std::sync::Mutex::new(session),
        player_cfg,
        spotify,
        spotify_tokens: Default::default(),
        cmd_tx: cmd_tx.clone(),
        playlist_file: cli.playlist_path.clone(),
        start_time: Arc::new(Mutex::new(None)),
        position_ms: Arc::new(Mutex::new(None)),
        tx_pcm,
        tx_mp3,
        stream_headers,
        playback_timeline,
        ogg_page_index,
        tx_lyrics_ws,
        current_lyrics,
        cli: cli.clone(),
        total_samples,
        stream_sessions: Arc::new(Mutex::new(std::collections::HashMap::new())),
    });

    // 5.5) Refresh only the Web API token based on its advertised expiry.
    // The reusable librespot playback session has an independent lifecycle.
    let state_for_refresh = state.clone();
    tokio::spawn(async move {
        loop {
            let mut sleep_duration = std::time::Duration::from_secs(45 * 60);

            if let Some(token) = state_for_refresh
                .spotify
                .token
                .lock()
                .await
                .unwrap()
                .as_ref()
            {
                if let Some(expires_at) = token.expires_at {
                    let now = chrono::Utc::now();
                    let diff = expires_at.signed_duration_since(now);
                    // Refresh 5 minutes (300 seconds) before expiration
                    let target_secs = diff.num_seconds() - 300;
                    if target_secs > 0 {
                        sleep_duration = std::time::Duration::from_secs(target_secs as u64);
                    } else {
                        // Already within 5 minutes or expired; refresh in 5 seconds
                        sleep_duration = std::time::Duration::from_secs(5);
                    }
                }
            }

            println!(
                "[OAuth/Web API] Next token refresh scheduled in {:.1} minutes.",
                sleep_duration.as_secs_f64() / 60.0
            );
            tokio::time::sleep(sleep_duration).await;

            println!("[OAuth/Web API] Background token refresh started.");
            if let Err(e) = state_for_refresh.refresh_spotify_token().await {
                eprintln!("[OAuth/Web API] ⚠️ Failed periodic token refresh: {e}");
                tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            }
        }
    });

    // 6) Spawn the playback manager
    tokio::spawn(playback_manager(cmd_rx, cli.clone(), state.clone()));
    println!("-> Playback manager spawned.");

    // 7) Define endpoint routes that are configurable at runtime.
    let stream_endpoint = cli.stream_endpoint.clone();
    let stream_route = warp::get()
        .and(endpoint_path_filter(stream_endpoint))
        .and(warp::query::<crate::models::StreamQuery>())
        .and(state::with_state(state.clone()))
        .and_then(handle_stream);

    let player_endpoint = cli.player_endpoint.clone();
    let player_route = endpoint_path_filter(player_endpoint.clone())
        .and(warp::get())
        .and(state::with_state(state.clone()))
        .and_then(handle_player);
    let minimal_player_route =
        endpoint_path_filter(join_endpoint_path(&player_endpoint, "minimal"))
            .and(warp::get())
            .and(state::with_state(state.clone()))
            .and_then(handle_minimal_player);
    let player_service_worker_route =
        endpoint_path_filter(join_endpoint_path(&player_endpoint, "spotifm-player-sw.js"))
            .and(warp::get())
            .and(state::with_state(state.clone()))
            .and_then(handle_player_service_worker);
    let player_audio_worklet_route = endpoint_path_filter(join_endpoint_path(
        &player_endpoint,
        "spotifm-audio-worklet.js",
    ))
    .and(warp::get())
    .and(state::with_state(state.clone()))
    .and_then(handle_player_audio_worklet);
    let player_vendor_route = endpoint_path_filter(join_endpoint_path(
        &player_endpoint,
        "player-assets/vendor/ogg-vorbis-decoder.min.js",
    ))
    .and(warp::get())
    .and(state::with_state(state.clone()))
    .and_then(handle_player_vendor_ogg_vorbis_decoder);
    let player_asset_route = warp::get()
        .and(player_asset_path_filter(player_endpoint))
        .and(state::with_state(state.clone()))
        .and_then(handle_player_asset_name);

    let routes = warp::path!("search" / String / u32)
        .and(warp::get())
        .and(warp::query::<crate::models::SearchQuery>())
        .and(check_permission("search", state.clone()))
        .and(state::with_state(state.clone()))
        .and_then(handle_search_category)
        .or(warp::path("np")
            .and(warp::get())
            .and(state::with_state(state.clone()))
            .and_then(handle_np))
        .or(warp::path("queue")
            .and(warp::path::end())
            .and(warp::get())
            .and(check_permission("queue", state.clone()))
            .and(state::with_state(state.clone()))
            .and_then(handle_queue_list))
        .or(warp::path!("queue" / String / String)
            .and(warp::get())
            .and(check_permission("queue", state.clone()))
            .and(state::with_state(state.clone()))
            .and_then(handle_queue_category))
        .or(warp::path!("queue" / String)
            .and(warp::get())
            .and(warp::query::<crate::models::SearchQuery>())
            .and(check_permission("queue", state.clone()))
            .and(state::with_state(state.clone()))
            .and_then(handle_queue_category_search))
        .or(warp::path!("queue" / String)
            .and(warp::delete())
            .and(check_permission("queue", state.clone()))
            .and(state::with_state(state.clone()))
            .and_then(handle_remove_queue_track))
        .or(warp::path("next")
            .and(warp::path::end())
            .and(warp::get())
            .and(check_permission("play", state.clone()))
            .and(state::with_state(state.clone()))
            .and_then(handle_next))
        .or(warp::path!("play" / String / String)
            .and(warp::get())
            .and(warp::query::<crate::models::AppendQuery>())
            .and(check_permission("play", state.clone()))
            .and(state::with_state(state.clone()))
            .and_then(handle_play_category))
        .or(warp::path!("play" / String)
            .and(warp::get())
            .and(warp::query::<crate::models::PlaySearchQuery>())
            .and(check_permission("play", state.clone()))
            .and(state::with_state(state.clone()))
            .and_then(handle_play_category_search))
        .or(warp::path!("playlist" / String / "add")
            .and(warp::get())
            .and(warp::query::raw())
            .and(check_permission("playlist", state.clone()))
            .and(state::with_state(state.clone()))
            .and_then(handle_playlist_add))
        .or(warp::path!("playlist" / String / "add")
            .and(warp::post())
            .and(warp::body::json())
            .and(check_permission("playlist", state.clone()))
            .and(state::with_state(state.clone()))
            .and_then(handle_playlist_add_json))
        .or(warp::path!("playlist" / String / "where")
            .and(warp::delete())
            .and(warp::query::raw())
            .and(check_permission("playlist", state.clone()))
            .and(state::with_state(state.clone()))
            .and_then(handle_playlist_delete_where))
        .or(warp::path!("playlist" / String / "where")
            .and(warp::post())
            .and(warp::body::json())
            .and(check_permission("playlist", state.clone()))
            .and(state::with_state(state.clone()))
            .and_then(handle_playlist_delete_where_json))
        .or(warp::path!("playlist" / "switch" / String)
            .and(warp::get())
            .and(check_permission("playlist", state.clone()))
            .and(state::with_state(state.clone()))
            .and_then(handle_playlist_switch))
        .or(warp::path!("playlist" / "track" / String / "play")
            .and(warp::get())
            .and(check_permission("play", state.clone()))
            .and(state::with_state(state.clone()))
            .and_then(handle_playlist_play_track))
        .or(warp::path!("playlist" / "shuffle" / String)
            .and(warp::get())
            .and(check_permission("playlist", state.clone()))
            .and(state::with_state(state.clone()))
            .and_then(handle_playlist_shuffle_named))
        .or(warp::path!("playlist" / "shuffle")
            .and(warp::get())
            .and(warp::path::end())
            .and(check_permission("playlist", state.clone()))
            .and(state::with_state(state.clone()))
            .and_then(handle_playlist_shuffle))
        .or(warp::path!("playlist" / String / "sort")
            .and(warp::get())
            .and(warp::query::<crate::models::PlaylistSortQuery>())
            .and(check_permission("playlist", state.clone()))
            .and(state::with_state(state.clone()))
            .and_then(handle_playlist_sort_named))
        .or(warp::path!("playlist" / "sort")
            .and(warp::get())
            .and(warp::path::end())
            .and(warp::query::<crate::models::PlaylistSortQuery>())
            .and(check_permission("playlist", state.clone()))
            .and(state::with_state(state.clone()))
            .and_then(handle_playlist_sort))
        .or(warp::path!("playlist" / String)
            .and(warp::delete())
            .and(check_permission("playlist", state.clone()))
            .and(state::with_state(state.clone()))
            .and_then(handle_playlist_delete))
        .or(warp::path!("playlist" / String)
            .and(warp::get())
            .and(check_permission("playlist", state.clone()))
            .and(state::with_state(state.clone()))
            .and_then(handle_playlist_get))
        .or(warp::path!("skip" / i32)
            .and(warp::get())
            .and(check_permission("skip", state.clone()))
            .and(state::with_state(state.clone()))
            .and_then(handle_skip_n))
        .or(warp::path!("skip" / String)
            .and(warp::get())
            .and(check_permission("skip", state.clone()))
            .and(state::with_state(state.clone()))
            .and_then(handle_skip_category))
        .or(warp::path("playlist")
            .and(warp::path::end())
            .and(warp::get())
            .and(check_permission("playlist", state.clone()))
            .and(state::with_state(state.clone()))
            .and_then(handle_playlist))
        .or(warp::path("playlists")
            .and(warp::path::end())
            .and(warp::get())
            .and(check_permission("playlist", state.clone()))
            .and(state::with_state(state.clone()))
            .and_then(handle_playlists))
        .or(warp::path("skip")
            .and(warp::get())
            .and(warp::path::end())
            .and(check_permission("skip", state.clone()))
            .and(state::with_state(state.clone()))
            .and_then(handle_skip))
        .or(warp::path!("playlist" / "track" / String)
            .and(warp::delete())
            .and(check_permission("playlist", state.clone()))
            .and(state::with_state(state.clone()))
            .and_then(handle_playlist_remove_track))
        .or(warp::path("ws")
            .and(warp::path::end())
            .and(warp::ws())
            .and(warp::query::<crate::models::WsQuery>())
            .and(state::with_state(state.clone()))
            .and_then(handle_lyrics_ws))
        .or(warp::path("lyrics")
            .and(warp::get())
            .and(warp::path::end())
            .and(state::with_state(state.clone()))
            .and_then(handle_lyrics_rest))
        .or(warp::path!("oauth" / "status")
            .and(warp::get())
            .and(state::with_state(state.clone()))
            .and_then(handle_oauth_status))
        .or(warp::path("oauth")
            .and(warp::path::end())
            .and(warp::get())
            .and_then(handle_oauth_page))
        .or(warp::path!("api" / "privs")
            .and(warp::get())
            .and(warp::header::optional::<String>("x-api-key"))
            .and(warp::header::optional::<String>("authorization"))
            .and(
                warp::query::<std::collections::HashMap<String, String>>().or_else(|_| async {
                    Ok::<(std::collections::HashMap<String, String>,), std::convert::Infallible>((
                        std::collections::HashMap::new(),
                    ))
                }),
            )
            .and(state::with_state(state.clone()))
            .and_then(handle_privs))
        .or(player_route)
        .or(minimal_player_route)
        .or(player_service_worker_route)
        .or(player_audio_worklet_route)
        .or(player_vendor_route)
        .or(stream_route)
        .or(player_asset_route);

    let routes = routes.recover(handle_rejection);

    let http_addr = format!("{}:{}", cli.api_ip, cli.api_port).parse::<SocketAddr>()?;
    let https_addr = format!("{}:{}", cli.api_ip, cli.tls_port).parse::<SocketAddr>()?;
    println!("-> HTTP server running at http://{http_addr}");
    println!("-> HTTPS server running at https://{https_addr}");

    let http_server = warp::serve(routes.clone()).run(http_addr);
    let https_server = warp::serve(routes)
        .tls()
        .cert_path(&tls_files.cert)
        .key_path(&tls_files.key)
        .run(https_addr);
    tokio::join!(http_server, https_server);

    Ok(())
}

fn endpoint_path_filter(
    endpoint: String,
) -> impl Filter<Extract = (), Error = warp::Rejection> + Clone {
    let endpoint = normalize_endpoint_path(&endpoint, "/");
    warp::path::full()
        .and_then(move |path: warp::path::FullPath| {
            let endpoint = endpoint.clone();
            async move {
                let request_path = normalize_endpoint_path(path.as_str(), "/");
                if request_path == endpoint {
                    Ok(())
                } else {
                    Err(warp::reject::not_found())
                }
            }
        })
        .untuple_one()
}

#[cfg(test)]
mod oauth_gateway_tests {
    use super::*;

    fn routes() -> (
        impl Filter<Extract = (warp::http::Response<String>,), Error = std::convert::Infallible> + Clone,
        tokio::sync::mpsc::Receiver<String>,
    ) {
        let (code_tx, code_rx) = tokio::sync::mpsc::channel(1);
        (
            oauth_gateway_routes(
                "https://accounts.spotify.test/authorize".to_string(),
                "/login".to_string(),
                "expected-state".to_string(),
                code_tx,
                "Playback".to_string(),
            ),
            code_rx,
        )
    }

    #[tokio::test]
    async fn oauth_endpoint_redirects_to_the_active_authorization() {
        let (routes, _) = routes();
        let response = warp::test::request().path("/oauth").reply(&routes).await;

        assert_eq!(response.status(), warp::http::StatusCode::FOUND);
        assert_eq!(
            response.headers().get("location").unwrap(),
            "https://accounts.spotify.test/authorize"
        );
    }

    #[tokio::test]
    async fn other_pre_auth_routes_redirect_to_oauth() {
        let (routes, _) = routes();
        let response = warp::test::request().path("/listen").reply(&routes).await;

        assert_eq!(response.status(), warp::http::StatusCode::FOUND);
        assert_eq!(response.headers().get("location").unwrap(), "/oauth");
    }

    #[tokio::test]
    async fn callback_validates_state_and_forwards_the_code() {
        let (routes, mut code_rx) = routes();
        let pending = warp::test::request()
            .path("/oauth/status")
            .reply(&routes)
            .await;
        assert_eq!(pending.status(), warp::http::StatusCode::OK);
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(pending.body()).unwrap()["status"],
            "authorization_required"
        );

        let rejected = warp::test::request()
            .path("/login?code=wrong&state=wrong-state")
            .reply(&routes)
            .await;
        assert_eq!(rejected.status(), warp::http::StatusCode::BAD_REQUEST);

        let accepted = warp::test::request()
            .path("/login?code=oauth-code&state=expected-state")
            .reply(&routes)
            .await;
        assert_eq!(accepted.status(), warp::http::StatusCode::OK);
        assert_eq!(code_rx.recv().await.as_deref(), Some("oauth-code"));

        let checking = warp::test::request()
            .path("/oauth/status")
            .reply(&routes)
            .await;
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(checking.body()).unwrap()["status"],
            "checking"
        );
    }
}

fn player_asset_path_filter(
    player_endpoint: String,
) -> impl Filter<Extract = (String,), Error = warp::Rejection> + Clone {
    let player_endpoint = normalize_endpoint_path(&player_endpoint, "/");
    warp::path::full().and_then(move |path: warp::path::FullPath| {
        let player_endpoint = player_endpoint.clone();
        async move {
            player_asset_name_for_path(path.as_str(), &player_endpoint)
                .ok_or_else(warp::reject::not_found)
        }
    })
}

fn player_asset_name_for_path(path: &str, player_endpoint: &str) -> Option<String> {
    if player_endpoint == "/" {
        let asset_name = path.trim_start_matches('/');
        return (!asset_name.is_empty()).then(|| asset_name.to_string());
    }

    let prefix = format!("{player_endpoint}/");
    path.strip_prefix(&prefix)
        .filter(|asset_name| !asset_name.is_empty())
        .map(ToString::to_string)
}

fn resolve_oauth_listener_addr(ip_or_host: &str, port: u16) -> Result<SocketAddr, Box<dyn Error>> {
    (ip_or_host, port).to_socket_addrs()?.next().ok_or_else(|| {
        format!(
            "Could not resolve OAuth listener address from host '{}' and port {}",
            ip_or_host, port
        )
        .into()
    })
}

fn open_browser(url: &str) {
    if std::process::Command::new("xdg-open")
        .arg(url)
        .status()
        .is_err()
    {
        let _ = std::process::Command::new("python3")
            .args(["-m", "webbrowser", url])
            .status();
    }
}

#[derive(Debug)]
enum AuthError {
    MissingKey,
    InvalidKey,
    PermissionDenied,
}

impl warp::reject::Reject for AuthError {}

fn check_permission(
    permission: &'static str,
    state: std::sync::Arc<state::AppState>,
) -> impl warp::Filter<Extract = (), Error = warp::Rejection> + Clone {
    warp::any()
        .and(warp::header::optional::<String>("x-api-key"))
        .and(warp::header::optional::<String>("authorization"))
        .and(
            warp::query::<std::collections::HashMap<String, String>>().or_else(|_| async {
                Ok::<(std::collections::HashMap<String, String>,), std::convert::Infallible>((
                    std::collections::HashMap::new(),
                ))
            }),
        )
        .and_then(
            move |x_api_key: Option<String>,
                  authorization: Option<String>,
                  query: std::collections::HashMap<String, String>| {
                let state = state.clone();
                async move {
                    if state.cli.api_keys.is_none()
                        || state.cli.api_keys.as_ref().unwrap().is_empty()
                    {
                        return Ok::<(), warp::Rejection>(());
                    }

                    let provided_key = if let Some(key) = x_api_key {
                        Some(key)
                    } else if let Some(auth) = authorization {
                        if auth.to_lowercase().starts_with("bearer ") {
                            Some(auth[7..].trim().to_string())
                        } else {
                            Some(auth.trim().to_string())
                        }
                    } else {
                        query.get("api_key").cloned()
                    };

                    let Some(key) = provided_key else {
                        return Err(warp::reject::custom(AuthError::MissingKey));
                    };

                    let api_keys = state.cli.api_keys.as_ref().unwrap();
                    let Some(permissions) = api_keys.get(&key) else {
                        return Err(warp::reject::custom(AuthError::InvalidKey));
                    };

                    if permissions.iter().any(|p| p == permission || p == "*") {
                        Ok::<(), warp::Rejection>(())
                    } else {
                        Err(warp::reject::custom(AuthError::PermissionDenied))
                    }
                }
            },
        )
        .untuple_one()
}

async fn handle_privs(
    x_api_key: Option<String>,
    authorization: Option<String>,
    query: std::collections::HashMap<String, String>,
    state: std::sync::Arc<state::AppState>,
) -> Result<impl warp::Reply, warp::Rejection> {
    #[derive(serde::Serialize)]
    struct AuthInfoResponse {
        auth_enabled: bool,
        authenticated: bool,
        permissions: Vec<String>,
    }

    let api_keys = &state.cli.api_keys;
    if api_keys.is_none() || api_keys.as_ref().unwrap().is_empty() {
        return Ok(warp::reply::json(&AuthInfoResponse {
            auth_enabled: false,
            authenticated: true,
            permissions: vec!["*".to_string()],
        }));
    }

    let provided_key = if let Some(key) = x_api_key {
        Some(key)
    } else if let Some(auth) = authorization {
        if auth.to_lowercase().starts_with("bearer ") {
            Some(auth[7..].trim().to_string())
        } else {
            Some(auth.trim().to_string())
        }
    } else {
        query.get("api_key").cloned()
    };

    let Some(key) = provided_key else {
        return Ok(warp::reply::json(&AuthInfoResponse {
            auth_enabled: true,
            authenticated: false,
            permissions: vec![],
        }));
    };

    let api_keys_map = api_keys.as_ref().unwrap();
    if let Some(permissions) = api_keys_map.get(&key) {
        Ok(warp::reply::json(&AuthInfoResponse {
            auth_enabled: true,
            authenticated: true,
            permissions: permissions.clone(),
        }))
    } else {
        Ok(warp::reply::json(&AuthInfoResponse {
            auth_enabled: true,
            authenticated: false,
            permissions: vec![],
        }))
    }
}

async fn handle_rejection(
    err: warp::Rejection,
) -> Result<impl warp::Reply, std::convert::Infallible> {
    use warp::http::StatusCode;

    let code;
    let message;

    if err.is_not_found() {
        code = StatusCode::NOT_FOUND;
        message = "NOT_FOUND";
    } else if let Some(auth_err) = err.find::<AuthError>() {
        match auth_err {
            AuthError::MissingKey => {
                code = StatusCode::UNAUTHORIZED;
                message = "API key missing. Use X-Api-Key header, Authorization Bearer token, or api_key query param.";
            }
            AuthError::InvalidKey => {
                code = StatusCode::FORBIDDEN;
                message = "Invalid API key.";
            }
            AuthError::PermissionDenied => {
                code = StatusCode::FORBIDDEN;
                message = "Permission denied for this API key.";
            }
        }
    } else if err.find::<warp::reject::MethodNotAllowed>().is_some() {
        code = StatusCode::METHOD_NOT_ALLOWED;
        message = "METHOD_NOT_ALLOWED";
    } else {
        eprintln!("unhandled rejection: {:?}", err);
        code = StatusCode::INTERNAL_SERVER_ERROR;
        message = "UNHANDLED_REJECTION";
    }

    let json = warp::reply::json(&serde_json::json!({
        "error": message,
        "code": code.as_u16(),
    }));

    Ok(warp::reply::with_status(json, code))
}
