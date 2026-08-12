use super::helpers::{
    is_valid_playlist_name, json_error, parse_raw_query, resolve_spotify_track_target,
    spotify_track_resolve_error_response, SpotifyTrackTarget,
};
use crate::audio::playback::{get_playlist_path, list_playlist_names};
use crate::models::{
    Command, PlaylistAddResult, PlaylistInfo, PlaylistSortQuery, PlaylistTracksBody, TrackItem,
};
use crate::state::AppState;
use futures_util::StreamExt;
use rspotify::clients::BaseClient;
use rspotify::prelude::Id;
use serde::Serialize;
use std::collections::HashSet;
use std::convert::Infallible;
use std::sync::Arc;
use tokio::sync::oneshot;
use warp::{http::StatusCode, Reply};

pub async fn handle_playlist(state: Arc<AppState>) -> Result<impl Reply, Infallible> {
    println!("[API] GET /playlist");
    let (tx, rx) = oneshot::channel();
    state.cmd_tx.send(Command::GetPlaylist { resp: tx }).ok();
    let list = rx.await.unwrap();
    Ok(warp::reply::with_status(
        warp::reply::json(&list),
        StatusCode::OK,
    ))
}

pub async fn handle_playlists(state: Arc<AppState>) -> Result<impl Reply, Infallible> {
    println!("[API] GET /playlists");
    match list_playlist_names(&state.playlist_file) {
        Ok(playlists) => {
            let mut detailed_playlists = Vec::new();
            for name in playlists {
                let path = get_playlist_path(&name, &state.playlist_file);
                let mut num_tracks = 0;
                let mut cover_urls = Vec::new();
                let mut artists = Vec::new();
                let mut last_modified = None;

                if path.exists() {
                    if let Ok(metadata) = std::fs::metadata(&path) {
                        if let Ok(modified) = metadata.modified() {
                            if let Ok(duration) =
                                modified.duration_since(std::time::SystemTime::UNIX_EPOCH)
                            {
                                last_modified = Some(duration.as_secs());
                            }
                        }
                    }

                    if let Ok(content) = std::fs::read_to_string(&path) {
                        if let Ok(tracks) = serde_json::from_str::<Vec<TrackItem>>(&content) {
                            num_tracks = tracks.len();

                            let mut unique_covers = HashSet::new();
                            for t in &tracks {
                                if let Some(ref cover) = t.cover_url {
                                    if !cover.is_empty() && unique_covers.insert(cover.clone()) {
                                        cover_urls.push(cover.clone());
                                        if cover_urls.len() >= 4 {
                                            break;
                                        }
                                    }
                                }
                            }

                            let mut unique_artists = HashSet::new();
                            for t in &tracks {
                                for artist in &t.artists {
                                    if !artist.is_empty() && unique_artists.insert(artist.clone()) {
                                        artists.push(artist.clone());
                                        if artists.len() >= 5 {
                                            break;
                                        }
                                    }
                                }
                                if artists.len() >= 5 {
                                    break;
                                }
                            }
                        }
                    }
                }

                detailed_playlists.push(PlaylistInfo {
                    name,
                    num_tracks,
                    cover_urls,
                    artists,
                    last_modified,
                });
            }
            Ok(warp::reply::with_status(
                warp::reply::json(&detailed_playlists),
                StatusCode::OK,
            ))
        }
        Err(e) => {
            #[derive(Serialize)]
            struct ErrResponse {
                error: String,
            }
            Ok(warp::reply::with_status(
                warp::reply::json(&ErrResponse { error: e }),
                StatusCode::INTERNAL_SERVER_ERROR,
            ))
        }
    }
}

pub async fn handle_playlist_add(
    playlist_name: String,
    raw_query: String,
    state: Arc<AppState>,
) -> Result<warp::reply::Response, Infallible> {
    println!("[API] GET /playlist/{}/add?{}", playlist_name, raw_query);

    if !is_valid_playlist_name(&playlist_name) {
        return Ok(json_error(
            StatusCode::BAD_REQUEST,
            "Playlist name must be alphanumeric",
        ));
    }

    let (tracks, albums, artists, playlists) = parse_raw_query(&raw_query);

    let client = state.spotify.clone();
    match resolve_spotify_track_target(
        &client,
        SpotifyTrackTarget::Lists {
            tracks: &tracks,
            albums: &albums,
            artists: &artists,
            playlists: &playlists,
        },
    )
    .await
    {
        Ok(resolved_tracks) => {
            Ok(add_resolved_tracks_to_playlist(playlist_name, resolved_tracks, state).await)
        }
        Err(error) => Ok(spotify_track_resolve_error_response(
            error,
            "Invalid playlist target category",
        )),
    }
}

pub async fn handle_playlist_add_json(
    playlist_name: String,
    body: PlaylistTracksBody,
    state: Arc<AppState>,
) -> Result<warp::reply::Response, Infallible> {
    println!("[API] POST /playlist/{}/add (JSON)", playlist_name);

    if !is_valid_playlist_name(&playlist_name) {
        return Ok(json_error(
            StatusCode::BAD_REQUEST,
            "Playlist name must be alphanumeric",
        ));
    }

    let client = state.spotify.clone();
    match resolve_spotify_track_target(
        &client,
        SpotifyTrackTarget::Lists {
            tracks: &body.tracks,
            albums: &body.albums,
            artists: &body.artists,
            playlists: &body.playlists,
        },
    )
    .await
    {
        Ok(resolved_tracks) => {
            Ok(add_resolved_tracks_to_playlist(playlist_name, resolved_tracks, state).await)
        }
        Err(error) => Ok(spotify_track_resolve_error_response(
            error,
            "Invalid playlist target category",
        )),
    }
}

async fn add_resolved_tracks_to_playlist(
    playlist_name: String,
    tracks: Vec<TrackItem>,
    state: Arc<AppState>,
) -> warp::reply::Response {
    let (tx, rx) = oneshot::channel();
    state
        .cmd_tx
        .send(Command::AddTracks {
            playlist_name,
            tracks,
            resp: tx,
        })
        .ok();

    match rx.await.unwrap() {
        Ok(PlaylistAddResult {
            added_tracks,
            total_tracks,
        }) => {
            #[derive(Serialize)]
            struct OkResp {
                status: &'static str,
                added_tracks: usize,
                total_tracks: usize,
            }
            warp::reply::with_status(
                warp::reply::json(&OkResp {
                    status: "success",
                    added_tracks,
                    total_tracks,
                }),
                StatusCode::OK,
            )
            .into_response()
        }
        Err(e) => json_error(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

pub async fn handle_playlist_delete_where(
    playlist_name: String,
    raw_query: String,
    state: Arc<AppState>,
) -> Result<impl Reply, Infallible> {
    println!(
        "[API] DELETE /playlist/{}/where?{}",
        playlist_name, raw_query
    );

    if !is_valid_playlist_name(&playlist_name) {
        #[derive(Serialize)]
        struct ErrResponse {
            error: &'static str,
        }
        return Ok(warp::reply::with_status(
            warp::reply::json(&ErrResponse {
                error: "Playlist name must be alphanumeric",
            }),
            StatusCode::BAD_REQUEST,
        ));
    }

    let (tracks, albums, artists, playlists) = parse_raw_query(&raw_query);

    // Resolve remote playlist tracks if any to track IDs
    let mut playlist_tracks = Vec::new();
    let client = state.spotify.clone();
    for p_id in playlists {
        let pid = match rspotify::model::PlaylistId::from_id(&p_id) {
            Ok(p) => p,
            Err(e) => {
                #[derive(Serialize)]
                struct ErrResponse {
                    error: String,
                }
                return Ok(warp::reply::with_status(
                    warp::reply::json(&ErrResponse {
                        error: format!("Invalid playlist ID '{}': {}", p_id, e),
                    }),
                    StatusCode::BAD_REQUEST,
                ));
            }
        };
        let mut stream = client.playlist_items(pid, None, None);
        while let Some(item_res) = stream.next().await {
            if let Ok(pi) = item_res {
                if let Some(rspotify::model::PlayableItem::Track(track)) = pi.track {
                    if let Some(id) = track.id {
                        playlist_tracks.push(id.id().to_string());
                    }
                }
            }
        }
    }

    let (tx, rx) = oneshot::channel();
    state
        .cmd_tx
        .send(Command::DeleteWhere {
            playlist_name,
            tracks,
            albums,
            artists,
            playlist_tracks,
            resp: tx,
        })
        .ok();

    match rx.await.unwrap() {
        Ok(()) => {
            #[derive(Serialize)]
            struct OkResp {
                status: &'static str,
            }
            Ok(warp::reply::with_status(
                warp::reply::json(&OkResp { status: "success" }),
                StatusCode::OK,
            ))
        }
        Err(e) => {
            #[derive(Serialize)]
            struct ErrResponse {
                error: String,
            }
            Ok(warp::reply::with_status(
                warp::reply::json(&ErrResponse { error: e }),
                StatusCode::INTERNAL_SERVER_ERROR,
            ))
        }
    }
}

pub async fn handle_playlist_delete_where_json(
    playlist_name: String,
    body: PlaylistTracksBody,
    state: Arc<AppState>,
) -> Result<impl Reply, Infallible> {
    println!("[API] POST /playlist/{}/where (JSON)", playlist_name);

    if !is_valid_playlist_name(&playlist_name) {
        #[derive(Serialize)]
        struct ErrResponse {
            error: &'static str,
        }
        return Ok(warp::reply::with_status(
            warp::reply::json(&ErrResponse {
                error: "Playlist name must be alphanumeric",
            }),
            StatusCode::BAD_REQUEST,
        ));
    }

    // Resolve remote playlist tracks if any to track IDs
    let mut playlist_tracks = Vec::new();
    let client = state.spotify.clone();
    for p_id in body.playlists {
        let pid = match rspotify::model::PlaylistId::from_id(&p_id) {
            Ok(p) => p,
            Err(e) => {
                #[derive(Serialize)]
                struct ErrResponse {
                    error: String,
                }
                return Ok(warp::reply::with_status(
                    warp::reply::json(&ErrResponse {
                        error: format!("Invalid playlist ID '{}': {}", p_id, e),
                    }),
                    StatusCode::BAD_REQUEST,
                ));
            }
        };
        let mut stream = client.playlist_items(pid, None, None);
        while let Some(item_res) = stream.next().await {
            if let Ok(pi) = item_res {
                if let Some(rspotify::model::PlayableItem::Track(track)) = pi.track {
                    if let Some(id) = track.id {
                        playlist_tracks.push(id.id().to_string());
                    }
                }
            }
        }
    }

    let (tx, rx) = oneshot::channel();
    state
        .cmd_tx
        .send(Command::DeleteWhere {
            playlist_name,
            tracks: body.tracks,
            albums: body.albums,
            artists: body.artists,
            playlist_tracks,
            resp: tx,
        })
        .ok();

    match rx.await.unwrap() {
        Ok(()) => {
            #[derive(Serialize)]
            struct OkResp {
                status: &'static str,
            }
            Ok(warp::reply::with_status(
                warp::reply::json(&OkResp { status: "success" }),
                StatusCode::OK,
            ))
        }
        Err(e) => {
            #[derive(Serialize)]
            struct ErrResponse {
                error: String,
            }
            Ok(warp::reply::with_status(
                warp::reply::json(&ErrResponse { error: e }),
                StatusCode::INTERNAL_SERVER_ERROR,
            ))
        }
    }
}

pub async fn handle_playlist_get(
    playlist_name: String,
    state: Arc<AppState>,
) -> Result<impl Reply, Infallible> {
    println!("[API] GET /playlist/{}", playlist_name);

    if !is_valid_playlist_name(&playlist_name) {
        #[derive(Serialize)]
        struct ErrResponse {
            error: &'static str,
        }
        return Ok(warp::reply::with_status(
            warp::reply::json(&ErrResponse {
                error: "Playlist name must be alphanumeric",
            }),
            StatusCode::BAD_REQUEST,
        ));
    }

    let path = get_playlist_path(&playlist_name, &state.playlist_file);
    if !path.exists() {
        #[derive(Serialize)]
        struct ErrResponse {
            error: &'static str,
        }
        return Ok(warp::reply::with_status(
            warp::reply::json(&ErrResponse {
                error: "Playlist not found",
            }),
            StatusCode::NOT_FOUND,
        ));
    }

    match std::fs::read_to_string(&path) {
        Ok(content) => match serde_json::from_str::<Vec<TrackItem>>(&content) {
            Ok(tracks) => Ok(warp::reply::with_status(
                warp::reply::json(&tracks),
                StatusCode::OK,
            )),
            Err(e) => {
                #[derive(Serialize)]
                struct ErrResponse {
                    error: String,
                }
                Ok(warp::reply::with_status(
                    warp::reply::json(&ErrResponse {
                        error: format!("Failed to parse playlist file: {}", e),
                    }),
                    StatusCode::INTERNAL_SERVER_ERROR,
                ))
            }
        },
        Err(e) => {
            #[derive(Serialize)]
            struct ErrResponse {
                error: String,
            }
            Ok(warp::reply::with_status(
                warp::reply::json(&ErrResponse {
                    error: format!("Failed to read playlist file: {}", e),
                }),
                StatusCode::INTERNAL_SERVER_ERROR,
            ))
        }
    }
}

pub async fn handle_playlist_delete(
    playlist_name: String,
    state: Arc<AppState>,
) -> Result<impl Reply, Infallible> {
    println!("[API] DELETE /playlist/{}", playlist_name);

    if !is_valid_playlist_name(&playlist_name) {
        #[derive(Serialize)]
        struct ErrResponse {
            error: &'static str,
        }
        return Ok(warp::reply::with_status(
            warp::reply::json(&ErrResponse {
                error: "Playlist name must be alphanumeric",
            }),
            StatusCode::BAD_REQUEST,
        ));
    }

    let (tx, rx) = oneshot::channel();
    state
        .cmd_tx
        .send(Command::DeletePlaylist {
            playlist_name,
            resp: tx,
        })
        .ok();

    match rx.await.unwrap() {
        Ok(()) => {
            #[derive(Serialize)]
            struct OkResp {
                status: &'static str,
            }
            Ok(warp::reply::with_status(
                warp::reply::json(&OkResp { status: "success" }),
                StatusCode::OK,
            ))
        }
        Err(e) => {
            #[derive(Serialize)]
            struct ErrResponse {
                error: String,
            }
            Ok(warp::reply::with_status(
                warp::reply::json(&ErrResponse { error: e }),
                StatusCode::INTERNAL_SERVER_ERROR,
            ))
        }
    }
}

pub async fn handle_playlist_switch(
    playlist_name: String,
    state: Arc<AppState>,
) -> Result<impl Reply, Infallible> {
    println!("[API] GET /playlist/switch/{}", playlist_name);

    if !is_valid_playlist_name(&playlist_name) {
        #[derive(Serialize)]
        struct ErrResponse {
            error: &'static str,
        }
        return Ok(warp::reply::with_status(
            warp::reply::json(&ErrResponse {
                error: "Playlist name must be alphanumeric and non-empty",
            }),
            StatusCode::BAD_REQUEST,
        ));
    }

    let (tx, rx) = oneshot::channel();
    state
        .cmd_tx
        .send(Command::SwitchPlaylist {
            name: playlist_name,
            resp: tx,
        })
        .ok();

    match rx.await.unwrap() {
        Ok(tracks) => Ok(warp::reply::with_status(
            warp::reply::json(&tracks),
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

pub async fn handle_playlist_play_track(
    track_id: String,
    state: Arc<AppState>,
) -> Result<warp::reply::Response, Infallible> {
    println!("[API] GET /playlist/track/{}/play", track_id);

    let (tx, rx) = oneshot::channel();
    state
        .cmd_tx
        .send(Command::PlayPlaylistTrack { track_id, resp: tx })
        .ok();

    match rx.await {
        Ok(Ok(now_playing)) => Ok(warp::reply::with_status(
            warp::reply::json(&now_playing),
            StatusCode::OK,
        )
        .into_response()),
        Ok(Err(error)) => Ok(json_error(StatusCode::NOT_FOUND, &error)),
        Err(_) => Ok(json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Playback manager did not respond",
        )),
    }
}

async fn handle_playlist_shuffle_inner(
    playlist_name: Option<String>,
    state: Arc<AppState>,
) -> Result<impl Reply, Infallible> {
    if let Some(ref name) = playlist_name {
        if !is_valid_playlist_name(name) {
            #[derive(Serialize)]
            struct ErrResponse {
                error: &'static str,
            }
            return Ok(warp::reply::with_status(
                warp::reply::json(&ErrResponse {
                    error: "Playlist name must be alphanumeric",
                }),
                StatusCode::BAD_REQUEST,
            ));
        }
    }

    let (tx, rx) = oneshot::channel();
    state
        .cmd_tx
        .send(Command::ShufflePlaylist {
            name: playlist_name,
            resp: tx,
        })
        .ok();

    match rx.await.unwrap() {
        Ok(tracks) => Ok(warp::reply::with_status(
            warp::reply::json(&tracks),
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

pub async fn handle_playlist_shuffle(state: Arc<AppState>) -> Result<impl Reply, Infallible> {
    println!("[API] GET /playlist/shuffle");
    handle_playlist_shuffle_inner(None, state).await
}

pub async fn handle_playlist_shuffle_named(
    playlist_name: String,
    state: Arc<AppState>,
) -> Result<impl Reply, Infallible> {
    println!("[API] GET /playlist/shuffle/{}", playlist_name);
    handle_playlist_shuffle_inner(Some(playlist_name), state).await
}

async fn handle_playlist_sort_inner(
    playlist_name: Option<String>,
    query: PlaylistSortQuery,
    state: Arc<AppState>,
) -> Result<impl Reply, Infallible> {
    if let Some(ref name) = playlist_name {
        if !is_valid_playlist_name(name) {
            #[derive(Serialize)]
            struct ErrResponse {
                error: &'static str,
            }
            return Ok(warp::reply::with_status(
                warp::reply::json(&ErrResponse {
                    error: "Playlist name must be alphanumeric",
                }),
                StatusCode::BAD_REQUEST,
            ));
        }
    }

    let sort_by = query.by.trim().to_ascii_lowercase();
    if !matches!(sort_by.as_str(), "artist" | "album" | "playlist") {
        #[derive(Serialize)]
        struct ErrResponse {
            error: &'static str,
        }
        return Ok(warp::reply::with_status(
            warp::reply::json(&ErrResponse {
                error: "Sort key must be artist, album, or playlist",
            }),
            StatusCode::BAD_REQUEST,
        ));
    }

    let (tx, rx) = oneshot::channel();
    state
        .cmd_tx
        .send(Command::SortPlaylist {
            name: playlist_name,
            by: sort_by,
            resp: tx,
        })
        .ok();

    match rx.await.unwrap() {
        Ok(tracks) => Ok(warp::reply::with_status(
            warp::reply::json(&tracks),
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

pub async fn handle_playlist_sort(
    query: PlaylistSortQuery,
    state: Arc<AppState>,
) -> Result<impl Reply, Infallible> {
    println!("[API] GET /playlist/sort?by={}", query.by);
    handle_playlist_sort_inner(None, query, state).await
}

pub async fn handle_playlist_sort_named(
    playlist_name: String,
    query: PlaylistSortQuery,
    state: Arc<AppState>,
) -> Result<impl Reply, Infallible> {
    println!("[API] GET /playlist/{}/sort?by={}", playlist_name, query.by);
    handle_playlist_sort_inner(Some(playlist_name), query, state).await
}

pub async fn handle_playlist_remove_track(
    track_id: String,
    state: Arc<AppState>,
) -> Result<impl Reply, Infallible> {
    println!("[API] DELETE /playlist/track/{}", track_id);

    let (tx, rx) = oneshot::channel();
    state
        .cmd_tx
        .send(Command::RemoveTrack { track_id, resp: tx })
        .ok();

    match rx.await.unwrap() {
        Ok(tracks) => Ok(warp::reply::with_status(
            warp::reply::json(&tracks),
            StatusCode::OK,
        )),
        Err(e) => {
            #[derive(Serialize)]
            struct ErrResponse {
                error: String,
            }
            Ok(warp::reply::with_status(
                warp::reply::json(&ErrResponse { error: e }),
                StatusCode::INTERNAL_SERVER_ERROR,
            ))
        }
    }
}
