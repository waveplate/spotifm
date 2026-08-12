use crate::models::{Command, SearchQuery, TrackItem};
use crate::state::AppState;
use serde::Serialize;
use std::convert::Infallible;
use std::sync::Arc;
use tokio::sync::oneshot;
use warp::{http::StatusCode, Reply};

use super::helpers::{
    resolve_nonempty_spotify_track_target, spotify_track_resolve_error_response, SpotifyTrackTarget,
};

pub async fn handle_queue_list(state: Arc<AppState>) -> Result<impl Reply, Infallible> {
    println!("[API] GET /queue");
    let (tx, rx) = oneshot::channel();
    state.cmd_tx.send(Command::GetQueue { resp: tx }).ok();
    let list = rx.await.unwrap();
    Ok(warp::reply::with_status(
        warp::reply::json(&list),
        StatusCode::OK,
    ))
}

pub async fn handle_queue_category(
    category: String,
    id: String,
    state: Arc<AppState>,
) -> Result<warp::reply::Response, Infallible> {
    println!("[API] GET /queue/{category}/{id}");

    Ok(queue_category_by_id(category, id, state).await)
}

pub async fn handle_queue_category_search(
    category: String,
    query: SearchQuery,
    state: Arc<AppState>,
) -> Result<warp::reply::Response, Infallible> {
    println!("[API] GET /queue/{}?q={}", category, query.q);

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
        Ok(resolved_tracks) => Ok(queue_resolved_tracks(resolved_tracks, state).await),
        Err(error) => Ok(spotify_track_resolve_error_response(
            error,
            "Invalid queue category",
        )),
    }
}

async fn queue_category_by_id(
    category: String,
    id: String,
    state: Arc<AppState>,
) -> warp::reply::Response {
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
        Ok(resolved_tracks) => queue_resolved_tracks(resolved_tracks, state).await,
        Err(error) => spotify_track_resolve_error_response(error, "Invalid queue category"),
    }
}

async fn queue_resolved_tracks(
    tracks: Vec<TrackItem>,
    state: Arc<AppState>,
) -> warp::reply::Response {
    let (tx, rx) = oneshot::channel();
    state
        .cmd_tx
        .send(Command::QueueTracks { tracks, resp: tx })
        .ok();
    let now = rx.await.unwrap();

    warp::reply::with_status(warp::reply::json(&now), StatusCode::OK).into_response()
}

pub async fn handle_remove_queue_track(
    track_id: String,
    state: Arc<AppState>,
) -> Result<impl Reply, Infallible> {
    println!("[API] DELETE /queue/{}", track_id);
    let (tx, rx) = oneshot::channel();
    state
        .cmd_tx
        .send(Command::RemoveQueueTrack { track_id, resp: tx })
        .ok();

    match rx.await.unwrap() {
        Ok(queue) => Ok(warp::reply::with_status(
            warp::reply::json(&queue),
            StatusCode::OK,
        )),
        Err(e) => {
            #[derive(Serialize)]
            struct ErrResponse {
                error: String,
            }
            Ok(warp::reply::with_status(
                warp::reply::json(&ErrResponse { error: e }),
                StatusCode::NOT_FOUND,
            ))
        }
    }
}
