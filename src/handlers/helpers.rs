use crate::models::TrackItem;
use futures_util::stream::{self, StreamExt};
use rspotify::clients::BaseClient;
use rspotify::http::HttpError;
use rspotify::model::{
    AlbumType, FullTrack, Market, PlayableItem, SearchType, SimplifiedTrack, TrackId as RTrackId,
};
use rspotify::prelude::Id;
use rspotify::{ClientError, ClientResult};
use serde::Serialize;
use std::collections::HashSet;
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use url::form_urlencoded;
use warp::{http::StatusCode, Reply};

const SEARCH_MAX_ATTEMPTS: usize = 5;
const SEARCH_RETRY_BASE_DELAY_MS: u64 = 250;
const SEARCH_MIN_CANDIDATE_LIMIT: u32 = 5;
const METADATA_MAX_ATTEMPTS: usize = 5;
const METADATA_RETRY_BASE_DELAY_MS: u64 = 500;
const ARTIST_ALBUM_REQUEST_CONCURRENCY: usize = 2;
static STREAM_SESSION_COUNTER: AtomicU64 = AtomicU64::new(1);
static SPOTIFY_METADATA_REQUESTS: tokio::sync::Semaphore =
    tokio::sync::Semaphore::const_new(ARTIST_ALBUM_REQUEST_CONCURRENCY);

pub fn next_stream_session_id() -> String {
    let id = STREAM_SESSION_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("stream-{id}")
}

pub fn parse_raw_query(raw: &str) -> (Vec<String>, Vec<String>, Vec<String>, Vec<String>) {
    let mut tracks = Vec::new();
    let mut albums = Vec::new();
    let mut artists = Vec::new();
    let mut playlists = Vec::new();

    for (key, val) in form_urlencoded::parse(raw.as_bytes()) {
        let key_clean = key.strip_suffix("[]").unwrap_or(key.as_ref());
        match key_clean {
            "tracks" => tracks.push(val.into_owned()),
            "albums" => albums.push(val.into_owned()),
            "artists" => artists.push(val.into_owned()),
            "playlists" => playlists.push(val.into_owned()),
            _ => {}
        }
    }

    (tracks, albums, artists, playlists)
}

pub fn is_valid_playlist_name(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric())
}

pub fn json_error(status: StatusCode, error: impl Into<String>) -> warp::reply::Response {
    #[derive(Serialize)]
    struct ErrResponse {
        error: String,
    }

    warp::reply::with_status(
        warp::reply::json(&ErrResponse {
            error: error.into(),
        }),
        status,
    )
    .into_response()
}

pub fn invalid_playlist_name_response(name: &str) -> Option<warp::reply::Response> {
    if is_valid_playlist_name(name) {
        None
    } else {
        Some(json_error(
            StatusCode::BAD_REQUEST,
            "Playlist name must be alphanumeric",
        ))
    }
}

fn is_transient_spotify_error(err: &ClientError) -> bool {
    match err {
        ClientError::Http(http_err) => match http_err.as_ref() {
            HttpError::StatusCode(response) => {
                matches!(response.status().as_u16(), 429 | 502 | 503 | 504)
            }
            HttpError::Client(inner) => inner.is_connect() || inner.is_timeout(),
        },
        _ => false,
    }
}

pub fn status_code_for_client_error(err: &ClientError) -> StatusCode {
    match err {
        ClientError::Http(http_err) => match http_err.as_ref() {
            HttpError::StatusCode(response) => {
                StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY)
            }
            HttpError::Client(inner) if inner.is_connect() || inner.is_timeout() => {
                StatusCode::BAD_GATEWAY
            }
            HttpError::Client(_) => StatusCode::INTERNAL_SERVER_ERROR,
        },
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

fn status_code_for_resolve_error(error: &str) -> StatusCode {
    if error.starts_with("Invalid ") {
        StatusCode::BAD_REQUEST
    } else if error.contains("429 Too Many Requests") {
        StatusCode::TOO_MANY_REQUESTS
    } else if error.contains("401 Unauthorized") {
        StatusCode::UNAUTHORIZED
    } else if error.contains("403 Forbidden") {
        StatusCode::FORBIDDEN
    } else if error.contains("404 Not Found") {
        StatusCode::NOT_FOUND
    } else {
        StatusCode::BAD_GATEWAY
    }
}

pub enum SpotifyTrackTarget<'a> {
    CategoryId {
        category: &'a str,
        id: &'a str,
    },
    CategorySearch {
        category: &'a str,
        query: &'a str,
    },
    Lists {
        tracks: &'a [String],
        albums: &'a [String],
        artists: &'a [String],
        playlists: &'a [String],
    },
}

pub enum SpotifyTrackResolveError {
    InvalidCategory,
    NoSearchResults,
    NoTracksResolved,
    Search(ClientError),
    Resolve(String),
}

pub fn spotify_track_resolve_error_response(
    error: SpotifyTrackResolveError,
    invalid_category_error: &'static str,
) -> warp::reply::Response {
    match error {
        SpotifyTrackResolveError::InvalidCategory => {
            json_error(StatusCode::BAD_REQUEST, invalid_category_error)
        }
        SpotifyTrackResolveError::NoSearchResults => {
            json_error(StatusCode::NOT_FOUND, "No search results")
        }
        SpotifyTrackResolveError::NoTracksResolved => {
            json_error(StatusCode::BAD_REQUEST, "No tracks resolved")
        }
        SpotifyTrackResolveError::Search(err) => {
            eprintln!("[API] Search error: {err}");
            json_error(status_code_for_client_error(&err), err.to_string())
        }
        SpotifyTrackResolveError::Resolve(err) => {
            eprintln!("[API] Spotify metadata resolution failed: {err}");
            json_error(status_code_for_resolve_error(&err), err)
        }
    }
}

fn spotify_retry_delay(error: &ClientError, attempt: usize) -> Duration {
    if let ClientError::Http(http_error) = error {
        if let HttpError::StatusCode(response) = http_error.as_ref() {
            if response.status().as_u16() == 429 {
                if let Some(seconds) = response
                    .headers()
                    .get("retry-after")
                    .and_then(|value| value.to_str().ok())
                    .and_then(|value| value.parse::<u64>().ok())
                {
                    return Duration::from_secs(seconds.max(1));
                }
            }
        }
    }

    let multiplier = 1u64 << attempt.saturating_sub(1).min(4);
    Duration::from_millis(METADATA_RETRY_BASE_DELAY_MS * multiplier)
}

async fn spotify_metadata_with_retry<T, F, Fut>(operation: &str, mut request: F) -> ClientResult<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = ClientResult<T>>,
{
    for attempt in 1..=METADATA_MAX_ATTEMPTS {
        let permit = SPOTIFY_METADATA_REQUESTS
            .acquire()
            .await
            .expect("Spotify metadata semaphore must remain open");
        let result = request().await;
        drop(permit);

        match result {
            Ok(value) => return Ok(value),
            Err(error) if attempt < METADATA_MAX_ATTEMPTS && is_transient_spotify_error(&error) => {
                let delay = spotify_retry_delay(&error, attempt);
                eprintln!(
                    "[API] Spotify metadata transient failure while trying to {operation} (attempt {attempt}/{METADATA_MAX_ATTEMPTS}, retrying in {:.1}s): {error}",
                    delay.as_secs_f64()
                );
                tokio::time::sleep(delay).await;
            }
            Err(error) => return Err(error),
        }
    }

    unreachable!("metadata retry loop always returns on its final attempt")
}

pub async fn spotify_search_with_retry(
    client: &rspotify::AuthCodePkceSpotify,
    query: &str,
    search_type: SearchType,
    limit: u32,
) -> ClientResult<rspotify::model::SearchResult> {
    let mut last_err = None;
    // Spotify can return a different first result when asked for only one or two
    // items. Fetch a small, stable candidate window and let API handlers trim
    // their response to the caller's requested limit.
    let candidate_limit = spotify_search_candidate_limit(limit);

    for attempt in 1..=SEARCH_MAX_ATTEMPTS {
        match client
            .search(query, search_type, None, None, Some(candidate_limit), None)
            .await
        {
            Ok(result) => return Ok(result),
            Err(err) if attempt < SEARCH_MAX_ATTEMPTS && is_transient_spotify_error(&err) => {
                eprintln!(
                    "[API] Spotify search transient failure for type '{search_type:?}' and query '{query}' (attempt {attempt}/{SEARCH_MAX_ATTEMPTS}): {err}"
                );
                last_err = Some(err);
                tokio::time::sleep(Duration::from_millis(
                    SEARCH_RETRY_BASE_DELAY_MS * attempt as u64,
                ))
                .await;
            }
            Err(err) => return Err(err),
        }
    }

    Err(last_err.expect("search retry loop must capture the final transient error"))
}

fn spotify_search_candidate_limit(requested_limit: u32) -> u32 {
    requested_limit.max(SEARCH_MIN_CANDIDATE_LIMIT)
}

#[cfg(test)]
mod tests {
    use super::spotify_search_candidate_limit;

    #[test]
    fn small_spotify_searches_use_stable_candidate_window() {
        assert_eq!(spotify_search_candidate_limit(1), 5);
        assert_eq!(spotify_search_candidate_limit(3), 5);
        assert_eq!(spotify_search_candidate_limit(5), 5);
        assert_eq!(spotify_search_candidate_limit(20), 20);
    }
}

pub fn search_type_for_category(category: &str) -> Option<SearchType> {
    match category {
        "track" => Some(SearchType::Track),
        "album" => Some(SearchType::Album),
        "artist" => Some(SearchType::Artist),
        "playlist" => Some(SearchType::Playlist),
        _ => None,
    }
}

pub async fn search_first_spotify_id(
    client: &rspotify::AuthCodePkceSpotify,
    category: &str,
    query: &str,
) -> ClientResult<Option<String>> {
    let Some(search_type) = search_type_for_category(category) else {
        return Ok(None);
    };

    match spotify_search_with_retry(client, query, search_type, 1).await? {
        rspotify::model::SearchResult::Tracks(page) => Ok(page
            .items
            .into_iter()
            .find_map(|track| track.id.map(|id| id.id().to_string()))),
        rspotify::model::SearchResult::Albums(page) => Ok(page
            .items
            .into_iter()
            .find_map(|album| album.id.map(|id| id.id().to_string()))),
        rspotify::model::SearchResult::Artists(page) => Ok(page
            .items
            .into_iter()
            .next()
            .map(|artist| artist.id.id().to_string())),
        rspotify::model::SearchResult::Playlists(page) => Ok(page
            .items
            .into_iter()
            .next()
            .map(|playlist| playlist.id.id().to_string())),
        _ => Ok(None),
    }
}

#[derive(Clone)]
struct AlbumSeed {
    album_id: String,
    album_name: Option<String>,
    cover_url: Option<String>,
}

pub fn simplified_track_to_item(
    track: SimplifiedTrack,
    album_id: Option<String>,
    album_name: Option<String>,
    cover_url: Option<String>,
) -> Option<TrackItem> {
    let track_id = track.id?;
    let artists = track.artists;
    let artist_ids = artists
        .iter()
        .filter_map(|a| a.id.as_ref().map(|id| id.id().to_string()))
        .collect();
    let artist_names = artists.iter().map(|a| a.name.clone()).collect();

    Some(TrackItem {
        track_id: track_id.id().to_string(),
        track_name: track.name,
        artists: artist_names,
        queue_idx: None,
        artist_ids,
        album_id,
        album_name,
        cover_url,
        playlist_id: None,
        playlist_name: None,
    })
}

pub fn full_track_to_item(track: &FullTrack, fallback_track_id: Option<&str>) -> Option<TrackItem> {
    let track_id = track
        .id
        .as_ref()
        .map(|id| id.id().to_string())
        .or_else(|| fallback_track_id.map(str::to_owned))?;

    Some(TrackItem {
        track_id,
        track_name: track.name.clone(),
        artists: track.artists.iter().map(|a| a.name.clone()).collect(),
        queue_idx: None,
        artist_ids: track
            .artists
            .iter()
            .filter_map(|a| a.id.as_ref().map(|id| id.id().to_string()))
            .collect(),
        album_id: track.album.id.as_ref().map(|id| id.id().to_string()),
        album_name: Some(track.album.name.clone()),
        cover_url: track.album.images.first().map(|img| img.url.clone()),
        playlist_id: None,
        playlist_name: None,
    })
}

pub async fn resolve_album_tracks_by_id(
    client: &rspotify::AuthCodePkceSpotify,
    album_id_str: &str,
    album_name: Option<String>,
    cover_url: Option<String>,
) -> Result<Vec<TrackItem>, String> {
    let market = Some(Market::FromToken);
    let album_id = rspotify::model::AlbumId::from_id(album_id_str)
        .map_err(|e| format!("Invalid album id '{}': {}", album_id_str, e))?;

    let mut resolved = Vec::new();
    let mut offset = 0;
    let mut skipped_missing_id = 0usize;

    loop {
        let operation = format!("fetch album '{album_id_str}' tracks at offset {offset}");
        let page = spotify_metadata_with_retry(&operation, || {
            client.album_track_manual(album_id.clone(), market, Some(50), Some(offset))
        })
        .await
        .map_err(|e| format!("Failed to fetch album tracks for '{}': {}", album_id_str, e))?;

        let has_next = page.next.is_some();
        let page_len = page.items.len() as u32;
        if page_len == 0 {
            break;
        }

        for track in page.items {
            if let Some(item) = simplified_track_to_item(
                track,
                Some(album_id_str.to_string()),
                album_name.clone(),
                cover_url.clone(),
            ) {
                resolved.push(item);
            } else {
                skipped_missing_id += 1;
            }
        }

        if !has_next {
            break;
        }
        offset += page_len;
    }

    if skipped_missing_id > 0 {
        println!(
            "[API] Skipped {} album track(s) without Spotify IDs for album '{}'",
            skipped_missing_id, album_id_str
        );
    }

    Ok(resolved)
}

pub async fn resolve_playlist_tracks_by_id(
    client: &rspotify::AuthCodePkceSpotify,
    playlist_id_str: &str,
) -> Result<Vec<TrackItem>, String> {
    let market = Some(Market::FromToken);
    let playlist_id = rspotify::model::PlaylistId::from_id(playlist_id_str)
        .map_err(|e| format!("Invalid playlist id '{}': {}", playlist_id_str, e))?;

    let mut resolved = Vec::new();
    let mut skipped_unavailable = 0usize;
    let mut skipped_missing_id = 0usize;
    let full_playlist = client
        .playlist(playlist_id.clone(), None, market)
        .await
        .map_err(|e| format!("Failed to fetch playlist '{}': {}", playlist_id_str, e))?;
    let source_playlist_id = playlist_id_str.to_string();
    let source_playlist_name = full_playlist.name.clone();
    let process_items = |items: Vec<rspotify::model::PlaylistItem>,
                         resolved: &mut Vec<TrackItem>,
                         skipped_unavailable: &mut usize,
                         skipped_missing_id: &mut usize| {
        for playlist_item in items {
            if let Some(PlayableItem::Track(track)) = playlist_item.track {
                if let Some(mut item) = full_track_to_item(&track, None) {
                    item.playlist_id = Some(source_playlist_id.clone());
                    item.playlist_name = Some(source_playlist_name.clone());
                    resolved.push(item);
                } else {
                    *skipped_missing_id += 1;
                }
            } else {
                *skipped_unavailable += 1;
            }
        }
    };

    let first_page = full_playlist.tracks;
    println!(
        "[API] Playlist '{}' metadata reports total={} first_page_items={} next={}",
        playlist_id_str,
        first_page.total,
        first_page.items.len(),
        first_page.next.is_some()
    );
    let mut next_offset = first_page.offset + first_page.items.len() as u32;
    let mut has_next = first_page.next.is_some();
    process_items(
        first_page.items,
        &mut resolved,
        &mut skipped_unavailable,
        &mut skipped_missing_id,
    );

    while has_next {
        let page = client
            .playlist_items_manual(
                playlist_id.clone(),
                None,
                market,
                Some(100),
                Some(next_offset),
            )
            .await
            .map_err(|e| {
                format!(
                    "Failed to fetch playlist items for '{}': {}",
                    playlist_id_str, e
                )
            })?;

        let page_len = page.items.len() as u32;
        if page_len == 0 {
            break;
        }

        has_next = page.next.is_some();
        next_offset = page.offset + page_len;
        process_items(
            page.items,
            &mut resolved,
            &mut skipped_unavailable,
            &mut skipped_missing_id,
        );
    }

    if skipped_unavailable > 0 || skipped_missing_id > 0 {
        println!(
            "[API] Skipped {} unavailable playlist item(s) and {} playlist track(s) without IDs for '{}'",
            skipped_unavailable, skipped_missing_id, playlist_id_str
        );
    }

    Ok(resolved)
}

pub async fn resolve_artist_tracks_by_id(
    client: &rspotify::AuthCodePkceSpotify,
    artist_id_str: &str,
) -> Result<Vec<TrackItem>, String> {
    let market = Some(Market::FromToken);
    let artist_id = rspotify::model::ArtistId::from_id(artist_id_str)
        .map_err(|e| format!("Invalid artist id '{}': {}", artist_id_str, e))?;

    let include_groups = [AlbumType::Album, AlbumType::Single];
    let mut album_offset = 0;
    let mut seen_album_ids = HashSet::new();
    let mut albums = Vec::new();

    loop {
        let operation = format!("fetch artist '{artist_id_str}' albums at offset {album_offset}");
        let page = spotify_metadata_with_retry(&operation, || {
            client.artist_albums_manual(
                artist_id.clone(),
                include_groups,
                market,
                Some(50),
                Some(album_offset),
            )
        })
        .await
        .map_err(|e| {
            format!(
                "Failed to fetch artist albums for '{}': {}",
                artist_id_str, e
            )
        })?;

        let has_next = page.next.is_some();
        let page_len = page.items.len() as u32;
        if page_len == 0 {
            break;
        }

        for album in page.items {
            if let Some(album_id) = album.id {
                let album_id_str = album_id.id().to_string();
                if seen_album_ids.insert(album_id_str.clone()) {
                    albums.push(AlbumSeed {
                        album_id: album_id_str,
                        album_name: Some(album.name),
                        cover_url: album.images.first().map(|img| img.url.clone()),
                    });
                }
            }
        }

        if !has_next {
            break;
        }
        album_offset += page_len;
    }

    let album_count = albums.len();
    let per_album_tracks = stream::iter(albums.into_iter().map(|album| {
        let spotify = client.clone();
        async move {
            resolve_album_tracks_by_id(&spotify, &album.album_id, album.album_name, album.cover_url)
                .await
        }
    }))
    .buffered(ARTIST_ALBUM_REQUEST_CONCURRENCY)
    .collect::<Vec<_>>()
    .await;

    let mut resolved = Vec::new();
    let mut seen_track_ids = HashSet::new();
    let mut failed_albums = 0usize;
    let mut first_album_error = None;
    for album_result in per_album_tracks {
        match album_result {
            Ok(album_tracks) => {
                for track in album_tracks {
                    if seen_track_ids.insert(track.track_id.clone()) {
                        resolved.push(track);
                    }
                }
            }
            Err(error) => {
                failed_albums += 1;
                eprintln!(
                    "[API] Skipping an unavailable album while resolving artist '{artist_id_str}': {error}"
                );
                if first_album_error.is_none() {
                    first_album_error = Some(error);
                }
            }
        }
    }

    if resolved.is_empty() {
        if let Some(error) = first_album_error {
            return Err(format!(
                "Failed to resolve any tracks for artist '{artist_id_str}': {error}"
            ));
        }
    }

    println!(
        "[API] Resolved {} unique track(s) from {} album(s) for artist '{}' ({} album request(s) skipped)",
        resolved.len(),
        album_count,
        artist_id_str,
        failed_albums
    );

    Ok(resolved)
}

pub async fn resolve_spotify_tracks(
    client: &rspotify::AuthCodePkceSpotify,
    tracks: &[String],
    albums: &[String],
    artists: &[String],
    playlists: &[String],
) -> Result<Vec<TrackItem>, String> {
    let mut resolved = Vec::new();

    // 1. Resolve tracks
    for t_id in tracks {
        let rid =
            RTrackId::from_id(t_id).map_err(|e| format!("Invalid track id '{}': {}", t_id, e))?;
        let full = client
            .track(rid, Some(Market::FromToken))
            .await
            .map_err(|e| format!("Failed to fetch track '{}': {}", t_id, e))?;
        if let Some(item) = full_track_to_item(&full, Some(t_id)) {
            resolved.push(item);
        }
    }

    // 2. Resolve albums
    for a_id in albums {
        let aid = rspotify::model::AlbumId::from_id(a_id)
            .map_err(|e| format!("Invalid album id '{}': {}", a_id, e))?;
        let album = client
            .album(aid, Some(Market::FromToken))
            .await
            .map_err(|e| format!("Failed to fetch album '{}': {}", a_id, e))?;
        let album_tracks = resolve_album_tracks_by_id(
            client,
            a_id,
            Some(album.name),
            album.images.first().map(|img| img.url.clone()),
        )
        .await?;
        println!(
            "[API] Resolved {} track(s) from album '{}'",
            album_tracks.len(),
            a_id
        );
        resolved.extend(album_tracks);
    }

    // 3. Resolve artists
    for art_id_str in artists {
        let artist_tracks = resolve_artist_tracks_by_id(client, art_id_str).await?;
        resolved.extend(artist_tracks);
    }

    // 4. Resolve playlists
    for p_id in playlists {
        let playlist_tracks = resolve_playlist_tracks_by_id(client, p_id).await?;
        println!(
            "[API] Resolved {} track(s) from playlist '{}'",
            playlist_tracks.len(),
            p_id
        );
        resolved.extend(playlist_tracks);
    }

    Ok(resolved)
}

pub async fn resolve_spotify_tracks_for_category(
    client: &rspotify::AuthCodePkceSpotify,
    category: &str,
    id: &str,
) -> Result<Vec<TrackItem>, String> {
    let mut tracks_list = Vec::new();
    let mut albums_list = Vec::new();
    let mut artists_list = Vec::new();
    let mut playlists_list = Vec::new();

    match category {
        "track" => tracks_list.push(id.to_string()),
        "album" => albums_list.push(id.to_string()),
        "artist" => artists_list.push(id.to_string()),
        "playlist" => playlists_list.push(id.to_string()),
        _ => return Err("Invalid category".to_string()),
    }

    resolve_spotify_tracks(
        client,
        &tracks_list,
        &albums_list,
        &artists_list,
        &playlists_list,
    )
    .await
}

pub async fn resolve_spotify_track_target(
    client: &rspotify::AuthCodePkceSpotify,
    target: SpotifyTrackTarget<'_>,
) -> Result<Vec<TrackItem>, SpotifyTrackResolveError> {
    match target {
        SpotifyTrackTarget::CategoryId { category, id } => {
            if search_type_for_category(category).is_none() {
                return Err(SpotifyTrackResolveError::InvalidCategory);
            }

            resolve_spotify_tracks_for_category(client, category, id)
                .await
                .map_err(SpotifyTrackResolveError::Resolve)
        }
        SpotifyTrackTarget::CategorySearch { category, query } => {
            if search_type_for_category(category).is_none() {
                return Err(SpotifyTrackResolveError::InvalidCategory);
            }

            let id = match search_first_spotify_id(client, category, query).await {
                Ok(Some(id)) => id,
                Ok(None) => return Err(SpotifyTrackResolveError::NoSearchResults),
                Err(err) => return Err(SpotifyTrackResolveError::Search(err)),
            };

            resolve_spotify_tracks_for_category(client, category, &id)
                .await
                .map_err(SpotifyTrackResolveError::Resolve)
        }
        SpotifyTrackTarget::Lists {
            tracks,
            albums,
            artists,
            playlists,
        } => resolve_spotify_tracks(client, tracks, albums, artists, playlists)
            .await
            .map_err(SpotifyTrackResolveError::Resolve),
    }
}

pub async fn resolve_nonempty_spotify_track_target(
    client: &rspotify::AuthCodePkceSpotify,
    target: SpotifyTrackTarget<'_>,
) -> Result<Vec<TrackItem>, SpotifyTrackResolveError> {
    let tracks = resolve_spotify_track_target(client, target).await?;
    if tracks.is_empty() {
        Err(SpotifyTrackResolveError::NoTracksResolved)
    } else {
        Ok(tracks)
    }
}

pub fn get_stream_content_type(cli: &crate::Cli) -> &'static str {
    let p_lower = cli.gst_pipeline.to_lowercase();
    if p_lower.contains("oggmux") {
        "audio/ogg"
    } else if p_lower.contains("webmux") {
        "audio/webm"
    } else if p_lower.contains("wavenc") {
        "audio/wav"
    } else if p_lower.contains("aac") || p_lower.contains("mp4mux") {
        "audio/aac"
    } else {
        "audio/mpeg"
    }
}

pub fn get_state_stream_content_type(state: &crate::state::AppState) -> &'static str {
    if state.player_cfg.passthrough {
        "audio/ogg"
    } else {
        get_stream_content_type(&state.cli)
    }
}
