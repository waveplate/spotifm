use crate::models::{AppendQuery, Command, PlaySearchQuery};
use crate::state::AppState;
use serde::Serialize;
use std::convert::Infallible;
use std::sync::Arc;
use tokio::sync::oneshot;
use warp::{http::StatusCode, Reply};

use super::helpers::{
    invalid_playlist_name_response, resolve_nonempty_spotify_track_target,
    spotify_track_resolve_error_response, SpotifyTrackTarget,
};

fn validate_playlist_name(name: Option<&String>) -> Option<warp::reply::Response> {
    name.and_then(|name| invalid_playlist_name_response(name))
}

pub async fn handle_np(state: Arc<AppState>) -> Result<impl Reply, Infallible> {
    let (tx, rx) = oneshot::channel();
    state.cmd_tx.send(Command::GetNowPlaying { resp: tx }).ok();
    let mut now = rx.await.unwrap();

    let start_time_opt = *state.start_time.lock().unwrap();
    let position_ms_opt = *state.position_ms.lock().unwrap();

    let computed_position = match (position_ms_opt, start_time_opt) {
        (Some(pos), Some(start)) => {
            if now.status == "playing" {
                let elapsed = start.elapsed().as_millis() as u32;
                let computed = pos + elapsed;
                if let Some(duration) = now.track_duration_ms {
                    Some(computed.min(duration))
                } else {
                    Some(computed)
                }
            } else {
                Some(pos)
            }
        }
        (pos, _) => pos,
    };

    now.position_ms = computed_position;
    now.listeners = state.active_listener_count();

    Ok(warp::reply::with_status(
        warp::reply::json(&now),
        StatusCode::OK,
    ))
}

pub async fn handle_next(state: Arc<AppState>) -> Result<impl Reply, Infallible> {
    println!("[API] GET /next");
    let (tx, rx) = oneshot::channel();
    state.cmd_tx.send(Command::GetNext { resp: tx }).ok();
    match rx.await.unwrap() {
        Some(item) => Ok(warp::reply::with_status(
            warp::reply::json(&item),
            StatusCode::OK,
        )),
        None => {
            #[derive(Serialize)]
            struct Err {
                error: &'static str,
            }
            let err = Err {
                error: "No next track",
            };
            Ok(warp::reply::with_status(
                warp::reply::json(&err),
                StatusCode::NOT_FOUND,
            ))
        }
    }
}

pub async fn handle_play_category(
    category: String,
    id: String,
    query: AppendQuery,
    state: Arc<AppState>,
) -> Result<warp::reply::Response, Infallible> {
    println!(
        "[API] GET /play/{}/{}?playlist={:?}",
        category, id, query.playlist
    );

    Ok(play_category_by_id(category, id, query.playlist, state).await)
}

pub async fn handle_play_category_search(
    category: String,
    query: PlaySearchQuery,
    state: Arc<AppState>,
) -> Result<warp::reply::Response, Infallible> {
    println!(
        "[API] GET /play/{}?q={}&playlist={:?}",
        category, query.q, query.playlist
    );

    if let Some(response) = validate_playlist_name(query.playlist.as_ref()) {
        return Ok(response);
    }

    let client = state.spotify.clone();
    match resolve_nonempty_spotify_track_target(
        &client,
        SpotifyTrackTarget::CategorySearch {
            category: &category,
            query: &query.q,
        },
    )
    .await
    {
        Ok(resolved_tracks) => {
            Ok(play_resolved_tracks(resolved_tracks, query.playlist, state).await)
        }
        Err(error) => Ok(spotify_track_resolve_error_response(
            error,
            "Invalid play category",
        )),
    }
}

async fn play_category_by_id(
    category: String,
    id: String,
    playlist_name: Option<String>,
    state: Arc<AppState>,
) -> warp::reply::Response {
    if let Some(response) = validate_playlist_name(playlist_name.as_ref()) {
        return response;
    }

    let client = state.spotify.clone();
    match resolve_nonempty_spotify_track_target(
        &client,
        SpotifyTrackTarget::CategoryId {
            category: &category,
            id: &id,
        },
    )
    .await
    {
        Ok(resolved_tracks) => play_resolved_tracks(resolved_tracks, playlist_name, state).await,
        Err(error) => spotify_track_resolve_error_response(error, "Invalid play category"),
    }
}

async fn play_resolved_tracks(
    tracks: Vec<crate::models::TrackItem>,
    playlist_name: Option<String>,
    state: Arc<AppState>,
) -> warp::reply::Response {
    let (tx, rx) = oneshot::channel();
    state
        .cmd_tx
        .send(Command::PlayNowCustom {
            tracks,
            playlist_name,
            resp: tx,
        })
        .ok();

    let np = rx.await.unwrap();
    warp::reply::with_status(warp::reply::json(&np), StatusCode::OK).into_response()
}

pub async fn handle_skip_category(
    category: String,
    state: Arc<AppState>,
) -> Result<impl Reply, Infallible> {
    println!("[API] GET /skip/{}", category);

    if category != "album" && category != "artist" {
        #[derive(Serialize)]
        struct ErrResponse {
            error: &'static str,
        }
        return Ok(warp::reply::with_status(
            warp::reply::json(&ErrResponse {
                error: "Category must be 'album' or 'artist'",
            }),
            StatusCode::BAD_REQUEST,
        ));
    }

    let (tx, rx) = oneshot::channel();
    state
        .cmd_tx
        .send(Command::SkipCategory { category, resp: tx })
        .ok();

    let np = rx.await.unwrap();
    Ok(warp::reply::with_status(
        warp::reply::json(&np),
        StatusCode::OK,
    ))
}

pub async fn handle_skip(state: Arc<AppState>) -> Result<impl Reply, Infallible> {
    println!("[API] GET /skip");
    let (tx, rx) = oneshot::channel();
    state
        .cmd_tx
        .send(Command::SkipN { count: 1, resp: tx })
        .ok();
    let now = rx.await.unwrap();
    Ok(warp::reply::with_status(
        warp::reply::json(&now),
        StatusCode::OK,
    ))
}

pub async fn handle_skip_n(count: i32, state: Arc<AppState>) -> Result<impl Reply, Infallible> {
    println!("[API] GET /skip/{}", count);

    if count == 0 {
        #[derive(Serialize)]
        struct ErrResponse {
            error: &'static str,
        }
        return Ok(warp::reply::with_status(
            warp::reply::json(&ErrResponse {
                error: "Skip count must not be 0",
            }),
            StatusCode::BAD_REQUEST,
        ));
    }

    let (tx, rx) = oneshot::channel();
    state.cmd_tx.send(Command::SkipN { count, resp: tx }).ok();
    let now = rx.await.unwrap();

    Ok(warp::reply::with_status(
        warp::reply::json(&now),
        StatusCode::OK,
    ))
}
