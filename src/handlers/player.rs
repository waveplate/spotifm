use super::helpers::get_state_stream_content_type;
use crate::cli::join_endpoint_path;
use crate::state::AppState;
use std::convert::Infallible;
use std::path::Path;
use std::sync::Arc;
use warp::Reply;

fn valid_player_asset_name(asset_name: &str) -> bool {
    !asset_name.is_empty()
        && !asset_name.starts_with('/')
        && asset_name
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

fn player_asset_content_type(asset_name: &str) -> &'static str {
    match Path::new(asset_name)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "application/javascript; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "wasm" => "application/wasm",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "mp3" => "audio/mpeg",
        "ogg" => "audio/ogg",
        "wav" => "audio/wav",
        _ => "application/octet-stream",
    }
}

fn local_player_asset_path(player_path: &Path, asset_name: &str) -> Option<std::path::PathBuf> {
    if player_path.is_dir() {
        Some(player_path.join(asset_name))
    } else if asset_name == "index.html" {
        Some(player_path.to_path_buf())
    } else {
        player_path.parent().map(|parent| parent.join(asset_name))
    }
}

fn load_player_asset_bytes(
    state: &Arc<AppState>,
    asset_name: &str,
) -> Result<bytes::Bytes, String> {
    if !valid_player_asset_name(asset_name) {
        return Err(format!("Invalid player asset path: {}", asset_name));
    }

    let asset_path = local_player_asset_path(&state.cli.player, asset_name).ok_or_else(|| {
        format!(
            "Could not resolve local player asset path for {}",
            asset_name
        )
    })?;
    std::fs::read(&asset_path)
        .map(bytes::Bytes::from)
        .map_err(|e| {
            format!(
                "Failed to load player asset from {}: {}",
                asset_path.display(),
                e
            )
        })
}

fn load_player_asset_text(state: &Arc<AppState>, asset_name: &str) -> Result<String, String> {
    let asset_path = local_player_asset_path(&state.cli.player, asset_name).ok_or_else(|| {
        format!(
            "Could not resolve local player asset path for {}",
            asset_name
        )
    })?;
    std::fs::read_to_string(&asset_path).map_err(|e| {
        format!(
            "Failed to load player asset from {}: {}",
            asset_path.display(),
            e
        )
    })
}

pub async fn handle_player(state: Arc<AppState>) -> Result<impl Reply, Infallible> {
    serve_player_html_asset(state, "index.html").await
}

pub async fn handle_minimal_player(state: Arc<AppState>) -> Result<impl Reply, Infallible> {
    serve_player_html_asset(state, "minimal.html").await
}

async fn serve_player_html_asset(
    state: Arc<AppState>,
    asset_name: &str,
) -> Result<impl Reply, Infallible> {
    let stream_endpoint = &state.cli.stream_endpoint;
    let player_endpoint = &state.cli.player_endpoint;
    let player_assets_endpoint = join_endpoint_path(player_endpoint, "player-assets");
    let player_service_worker_endpoint =
        join_endpoint_path(player_endpoint, "spotifm-player-sw.js");
    let player_audio_worklet_endpoint =
        join_endpoint_path(player_endpoint, "spotifm-audio-worklet.js");
    let stream_is_ogg = get_state_stream_content_type(&state).contains("ogg");
    let stream_is_passthrough = state.player_cfg.passthrough;
    let raw_html = match load_player_asset_text(&state, asset_name) {
        Ok(content) => content,
        Err(e) => {
            let response = warp::http::Response::builder()
                .status(warp::http::StatusCode::INTERNAL_SERVER_ERROR)
                .header("content-type", "text/plain; charset=utf-8")
                .body(e)
                .unwrap();
            return Ok(response);
        }
    };

    // Replace placeholders used by the shared player template.
    let html = raw_html
        .replace("{listen_endpoint}", stream_endpoint)
        .replace("{stream_endpoint}", stream_endpoint)
        .replace("{player_endpoint}", player_endpoint)
        .replace("{player_assets_endpoint}", &player_assets_endpoint)
        .replace(
            "{player_service_worker_endpoint}",
            &player_service_worker_endpoint,
        )
        .replace(
            "{player_audio_worklet_endpoint}",
            &player_audio_worklet_endpoint,
        )
        .replace(
            "{stream_is_ogg}",
            if stream_is_ogg { "true" } else { "false" },
        )
        .replace(
            "{stream_is_passthrough}",
            if stream_is_passthrough {
                "true"
            } else {
                "false"
            },
        );

    let response = warp::http::Response::builder()
        .header("content-type", "text/html; charset=utf-8")
        .header(
            "cache-control",
            "no-store, no-cache, must-revalidate, max-age=0",
        )
        .header("pragma", "no-cache")
        .header("expires", "0")
        .body(html)
        .unwrap();

    Ok(response)
}

pub async fn handle_player_service_worker(state: Arc<AppState>) -> Result<impl Reply, Infallible> {
    match load_player_asset_text(&state, "spotifm-player-sw.js") {
        Ok(content) => Ok(warp::http::Response::builder()
            .header("content-type", "application/javascript; charset=utf-8")
            .header("service-worker-allowed", "/")
            .header(
                "cache-control",
                "no-store, no-cache, must-revalidate, max-age=0",
            )
            .header("pragma", "no-cache")
            .header("expires", "0")
            .body(content)
            .unwrap()),
        Err(e) => Ok(warp::http::Response::builder()
            .status(warp::http::StatusCode::INTERNAL_SERVER_ERROR)
            .header("content-type", "text/plain; charset=utf-8")
            .body(e)
            .unwrap()),
    }
}

pub async fn handle_player_audio_worklet(state: Arc<AppState>) -> Result<impl Reply, Infallible> {
    match load_player_asset_text(&state, "spotifm-audio-worklet.js") {
        Ok(content) => Ok(warp::http::Response::builder()
            .header("content-type", "application/javascript; charset=utf-8")
            .header(
                "cache-control",
                "no-store, no-cache, must-revalidate, max-age=0",
            )
            .header("pragma", "no-cache")
            .header("expires", "0")
            .body(content)
            .unwrap()),
        Err(e) => Ok(warp::http::Response::builder()
            .status(warp::http::StatusCode::INTERNAL_SERVER_ERROR)
            .header("content-type", "text/plain; charset=utf-8")
            .body(e)
            .unwrap()),
    }
}

pub async fn handle_player_vendor_ogg_vorbis_decoder(
    state: Arc<AppState>,
) -> Result<impl Reply, Infallible> {
    match load_player_asset_text(&state, "vendor/ogg-vorbis-decoder.min.js") {
        Ok(content) => Ok(warp::http::Response::builder()
            .header("content-type", "application/javascript; charset=utf-8")
            .header(
                "cache-control",
                "no-store, no-cache, must-revalidate, max-age=0",
            )
            .header("pragma", "no-cache")
            .header("expires", "0")
            .body(content)
            .unwrap()),
        Err(e) => Ok(warp::http::Response::builder()
            .status(warp::http::StatusCode::INTERNAL_SERVER_ERROR)
            .header("content-type", "text/plain; charset=utf-8")
            .body(e)
            .unwrap()),
    }
}

pub async fn handle_player_asset_name(
    asset_name: String,
    state: Arc<AppState>,
) -> Result<impl Reply, Infallible> {
    match load_player_asset_bytes(&state, &asset_name) {
        Ok(content) => Ok(warp::http::Response::builder()
            .header("content-type", player_asset_content_type(&asset_name))
            .header(
                "cache-control",
                "no-store, no-cache, must-revalidate, max-age=0",
            )
            .header("pragma", "no-cache")
            .header("expires", "0")
            .body(content)
            .unwrap()),
        Err(e) => Ok(warp::http::Response::builder()
            .status(warp::http::StatusCode::NOT_FOUND)
            .header("content-type", "text/plain; charset=utf-8")
            .body(bytes::Bytes::from(e))
            .unwrap()),
    }
}
