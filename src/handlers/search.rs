use super::helpers::{spotify_search_with_retry, status_code_for_client_error};
use crate::models::{ExtendedTrack, SearchQuery, SimpleAlbum, SimpleArtist, SimplePlaylist};
use crate::state::AppState;
use rspotify::model::{FullArtist, FullTrack, SearchType, SimplifiedAlbum, SimplifiedPlaylist};
use rspotify::prelude::Id;
use serde::Serialize;
use std::convert::Infallible;
use std::sync::Arc;
use warp::{http::StatusCode, Reply};

pub async fn handle_search_category(
    category: String,
    limit: u32,
    query: SearchQuery,
    state: Arc<AppState>,
) -> Result<impl Reply, Infallible> {
    println!("[API] GET /search/{}?q={}&limit={limit}", category, query.q);
    if !(1..=50).contains(&limit) {
        #[derive(Serialize)]
        struct ErrResponse {
            error: &'static str,
        }

        return Ok(warp::reply::with_status(
            warp::reply::json(&ErrResponse {
                error: "Search limit must be between 1 and 50",
            }),
            StatusCode::BAD_REQUEST,
        ));
    }

    let search_type = match category.as_str() {
        "track" => SearchType::Track,
        "album" => SearchType::Album,
        "artist" => SearchType::Artist,
        "playlist" => SearchType::Playlist,
        _ => {
            #[derive(Serialize)]
            struct ErrResponse {
                error: &'static str,
            }

            return Ok(warp::reply::with_status(
                warp::reply::json(&ErrResponse {
                    error: "Search category must be 'track', 'album', 'artist', or 'playlist'",
                }),
                StatusCode::BAD_REQUEST,
            ));
        }
    };

    if let Err(e) = state.ensure_spotify_token().await {
        eprintln!("[API] Search token refresh error: {e}");
        #[derive(Serialize)]
        struct ErrResponse {
            error: String,
        }
        return Ok(warp::reply::with_status(
            warp::reply::json(&ErrResponse {
                error: e.to_string(),
            }),
            status_code_for_client_error(&e),
        ));
    }

    match spotify_search_with_retry(&state.spotify, &query.q, search_type, limit).await {
        Ok(rspotify::model::SearchResult::Tracks(page)) => {
            let results: Vec<ExtendedTrack> = page
                .items
                .into_iter()
                .filter_map(|t: FullTrack| {
                    let id = t.id?;
                    Some(ExtendedTrack {
                        track_id: id.id().to_string(),
                        uri: id.uri().to_string(),
                        track_name: t.name.clone(),
                        duration_ms: t.duration.num_milliseconds() as i32,
                        explicit: t.explicit,
                        popularity: Some(t.popularity as i32),
                        artists: t.artists.iter().map(|a| a.name.clone()).collect(),
                        artist_ids: t
                            .artists
                            .iter()
                            .filter_map(|a| a.id.as_ref().map(|id| id.id().to_string()))
                            .collect(),
                        album_id: t.album.id.as_ref().map(|id| id.id().to_string()),
                        album_name: Some(t.album.name.clone()),
                        album_artists: t.album.artists.iter().map(|ar| ar.name.clone()).collect(),
                        cover_url: t
                            .album
                            .images
                            .iter()
                            .find(|img| img.height == Some(640))
                            .or_else(|| t.album.images.first())
                            .map(|img| img.url.clone()),
                        preview_url: t.preview_url.clone(),
                    })
                })
                .take(limit as usize)
                .collect();
            Ok(warp::reply::with_status(
                warp::reply::json(&results),
                StatusCode::OK,
            ))
        }
        Ok(rspotify::model::SearchResult::Albums(page)) => {
            let results: Vec<SimpleAlbum> = page
                .items
                .into_iter()
                .filter_map(|a: SimplifiedAlbum| {
                    let id = a.id.as_ref()?;
                    Some(SimpleAlbum {
                        album_id: id.id().to_string(),
                        uri: id.uri().to_string(),
                        album_name: a.name.clone(),
                        artists: a.artists.iter().map(|ar| ar.name.clone()).collect(),
                        artist_ids: a
                            .artists
                            .iter()
                            .filter_map(|ar| ar.id.as_ref().map(|id| id.id().to_string()))
                            .collect(),
                        cover_url: a.images.first().map(|img| img.url.clone()),
                    })
                })
                .take(limit as usize)
                .collect();
            Ok(warp::reply::with_status(
                warp::reply::json(&results),
                StatusCode::OK,
            ))
        }
        Ok(rspotify::model::SearchResult::Artists(page)) => {
            let results: Vec<SimpleArtist> = page
                .items
                .into_iter()
                .map(|ar: FullArtist| {
                    let id = &ar.id;
                    SimpleArtist {
                        artist_id: id.id().to_string(),
                        uri: id.uri().to_string(),
                        artist_name: ar.name.clone(),
                        genres: ar.genres.clone(),
                        popularity: Some(ar.popularity as i32),
                        cover_url: ar.images.first().map(|img| img.url.clone()),
                    }
                })
                .take(limit as usize)
                .collect();
            Ok(warp::reply::with_status(
                warp::reply::json(&results),
                StatusCode::OK,
            ))
        }
        Ok(rspotify::model::SearchResult::Playlists(page)) => {
            let results: Vec<SimplePlaylist> = page
                .items
                .into_iter()
                .map(|p: SimplifiedPlaylist| {
                    let id = &p.id;
                    SimplePlaylist {
                        playlist_id: id.id().to_string(),
                        uri: id.uri().to_string(),
                        playlist_name: p.name.clone(),
                        owner: p.owner.display_name.clone().unwrap_or_default(),
                        total_tracks: p.tracks.total,
                        cover_url: p.images.first().map(|img| img.url.clone()),
                    }
                })
                .take(limit as usize)
                .collect();
            Ok(warp::reply::with_status(
                warp::reply::json(&results),
                StatusCode::OK,
            ))
        }
        Err(e) => {
            eprintln!("[API] Search error: {e}");
            #[derive(Serialize)]
            struct ErrResponse {
                error: String,
            }
            Ok(warp::reply::with_status(
                warp::reply::json(&ErrResponse {
                    error: e.to_string(),
                }),
                status_code_for_client_error(&e),
            ))
        }
        _ => {
            eprintln!("[API] Unexpected search result type.");
            #[derive(Serialize)]
            struct ErrResponse {
                error: String,
            }
            Ok(warp::reply::with_status(
                warp::reply::json(&ErrResponse {
                    error: "Unexpected search result type".to_string(),
                }),
                StatusCode::INTERNAL_SERVER_ERROR,
            ))
        }
    }
}
