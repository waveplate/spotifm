//! Application catalogue models and the backup for Web API catalogue failures.
use crate::anonymous::{AnonymousSearch, FallbackError};
use crate::models::{ExtendedTrack, SimpleAlbum, SimpleArtist, SimplePlaylist, TrackItem};
use futures_util::{stream, StreamExt};
use librespot::core::{Session, SpotifyUri};
use librespot_metadata::{Album, Artist, Metadata, Playlist, Track};
use rspotify::model::SearchType;
use serde::Serialize;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, Semaphore};

#[derive(Clone, Serialize)]
#[serde(untagged)]
pub enum SearchItems {
    Tracks(Vec<ExtendedTrack>),
    Albums(Vec<SimpleAlbum>),
    Artists(Vec<SimpleArtist>),
    Playlists(Vec<SimplePlaylist>),
}

pub struct CatalogSearch {
    pub items: SearchItems,
    pub source: &'static str,
}

pub enum CatalogError {
    WebApi(rspotify::ClientError),
    Anonymous(FallbackError),
}

impl CatalogSearch {
    pub fn first_id(&self) -> Option<String> {
        match &self.items {
            SearchItems::Tracks(items) => items.first().map(|item| item.track_id.clone()),
            SearchItems::Albums(items) => items.first().map(|item| item.album_id.clone()),
            SearchItems::Artists(items) => items.first().map(|item| item.artist_id.clone()),
            SearchItems::Playlists(items) => items.first().map(|item| item.playlist_id.clone()),
        }
    }

    pub fn from_web(
        rows: &[Value],
        category: SearchType,
        limit: usize,
    ) -> Result<Self, rspotify::ClientError> {
        // Normalize JSON directly so removed or newly added provider fields do
        // not dictate Spotifm's public response structure.
        let names = |node: &Value| -> Vec<String> {
            node["artists"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|artist| text(artist, "name"))
                .collect()
        };
        let ids = |node: &Value| -> Vec<String> {
            node["artists"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|artist| text(artist, "id"))
                .collect()
        };
        let image = |node: &Value| text(&node["images"][0], "url");
        let popularity = |node: &Value| node["popularity"].as_i64().map(|n| n.clamp(0, 100) as i32);
        let items = match category {
            SearchType::Track => SearchItems::Tracks(
                rows.iter()
                    .filter_map(|t| {
                        let (id, uri) = node_id(t, "track")?;
                        Some(ExtendedTrack {
                            track_id: id,
                            uri,
                            track_name: text(t, "name")?,
                            duration_ms: t["duration_ms"]
                                .as_i64()
                                .unwrap_or(0)
                                .clamp(0, i32::MAX as i64)
                                as i32,
                            explicit: t["explicit"].as_bool().unwrap_or(false),
                            popularity: popularity(t),
                            artists: names(t),
                            artist_ids: ids(t),
                            album_id: text(&t["album"], "id"),
                            album_name: text(&t["album"], "name"),
                            album_artists: names(&t["album"]),
                            cover_url: t["album"]["images"].as_array().and_then(|images| {
                                images
                                    .iter()
                                    .find(|i| i["height"].as_u64() == Some(640))
                                    .or_else(|| images.first())
                                    .and_then(|i| text(i, "url"))
                            }),
                            preview_url: text(t, "preview_url"),
                        })
                    })
                    .take(limit)
                    .collect(),
            ),
            SearchType::Album => SearchItems::Albums(
                rows.iter()
                    .filter_map(|a| {
                        let (id, uri) = node_id(a, "album")?;
                        Some(SimpleAlbum {
                            album_id: id,
                            uri,
                            album_name: text(a, "name")?,
                            artists: names(a),
                            artist_ids: ids(a),
                            cover_url: image(a),
                        })
                    })
                    .take(limit)
                    .collect(),
            ),
            SearchType::Artist => SearchItems::Artists(
                rows.iter()
                    .filter_map(|a| {
                        let (id, uri) = node_id(a, "artist")?;
                        Some(SimpleArtist {
                            artist_id: id,
                            uri,
                            artist_name: text(a, "name")?,
                            genres: a["genres"]
                                .as_array()
                                .into_iter()
                                .flatten()
                                .filter_map(Value::as_str)
                                .map(str::to_owned)
                                .collect(),
                            popularity: popularity(a),
                            cover_url: image(a),
                        })
                    })
                    .take(limit)
                    .collect(),
            ),
            SearchType::Playlist => SearchItems::Playlists(
                rows.iter()
                    .filter_map(|p| {
                        let (id, uri) = node_id(p, "playlist")?;
                        Some(SimplePlaylist {
                            playlist_id: id,
                            uri,
                            playlist_name: text(p, "name")?,
                            owner: text(&p["owner"], "display_name").unwrap_or_default(),
                            total_tracks: p
                                .pointer("/items/total")
                                .and_then(Value::as_u64)
                                .or_else(|| p.pointer("/tracks/total").and_then(Value::as_u64))
                                .and_then(|total| u32::try_from(total).ok()),
                            cover_url: image(p),
                        })
                    })
                    .take(limit)
                    .collect(),
            ),
            _ => return Err(invalid_web_search("Unsupported catalogue search type")),
        };
        Ok(Self {
            items,
            source: "spotify_web_api",
        })
    }
}

pub fn invalid_web_search(message: &str) -> rspotify::ClientError {
    rspotify::ClientError::ParseJson(serde::de::Error::custom(message))
}

pub fn web_search_rows(
    response: &Value,
    category: SearchType,
) -> Result<&Vec<Value>, rspotify::ClientError> {
    let key = match category {
        SearchType::Track => "tracks",
        SearchType::Album => "albums",
        SearchType::Artist => "artists",
        SearchType::Playlist => "playlists",
        _ => return Err(invalid_web_search("Unsupported catalogue search type")),
    };
    response[key]["items"]
        .as_array()
        .ok_or_else(|| invalid_web_search("Spotify Web API returned an unexpected search response"))
}

fn unwrap_node(mut node: &Value) -> &Value {
    for _ in 0..4 {
        match ["track", "item", "data"]
            .into_iter()
            .find_map(|key| node.get(key).filter(|value| value.is_object()))
        {
            Some(child) => node = child,
            None => break,
        }
    }
    node
}

fn text(node: &Value, key: &str) -> Option<String> {
    node.get(key)?.as_str().map(str::to_owned)
}

fn node_id(node: &Value, category: &str) -> Option<(String, String)> {
    let uri = node.get("uri")?.as_str()?;
    let id = uri.strip_prefix(&format!("spotify:{category}:"))?;
    if id.len() != 22 || !id.bytes().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    Some((id.to_owned(), uri.to_owned()))
}

fn artist_names(node: &Value) -> Vec<String> {
    node.pointer("/artists/items")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|artist| text(&artist["profile"], "name").or_else(|| text(artist, "name")))
        .collect()
}

fn artist_ids(node: &Value) -> Vec<String> {
    node.pointer("/artists/items")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|artist| node_id(artist, "artist").map(|(id, _)| id))
        .collect()
}

fn cover(node: &Value) -> Option<String> {
    [
        "/album/coverArt/sources",
        "/coverArt/sources",
        "/visuals/avatarImage/sources",
        "/images/items/0/sources",
    ]
    .into_iter()
    .filter_map(|path| node.pointer(path).and_then(Value::as_array))
    .find_map(|sources| {
        sources
            .iter()
            .max_by_key(|source| source["height"].as_u64().unwrap_or(0))
            .and_then(|source| text(source, "url"))
    })
}

fn anonymous_results(
    search: &Value,
    category: SearchType,
    limit: usize,
) -> Result<SearchItems, FallbackError> {
    let keys: &[&str] = match category {
        SearchType::Track => &["tracksV2", "tracks"],
        SearchType::Album => &["albums"],
        SearchType::Artist => &["artists"],
        SearchType::Playlist => &["playlists"],
        _ => {
            return Err(FallbackError::protocol(
                "Unsupported anonymous search category",
            ))
        }
    };
    let rows = keys
        .iter()
        .find_map(|key| {
            search
                .get(key)
                .and_then(|section| section["items"].as_array())
        })
        .ok_or_else(|| {
            FallbackError::protocol(
                "Spotify anonymous search returned an unexpected category response",
            )
        })?;
    let nodes = || rows.iter().map(unwrap_node);
    Ok(match category {
        SearchType::Track => SearchItems::Tracks(
            nodes()
                .filter_map(|node| {
                    let (id, uri) = node_id(node, "track")?;
                    Some(ExtendedTrack {
                        track_id: id,
                        uri,
                        track_name: text(node, "name")?,
                        duration_ms: node
                            .pointer("/duration/totalMilliseconds")
                            .and_then(Value::as_i64)
                            .unwrap_or(0)
                            .clamp(0, i32::MAX as i64) as i32,
                        explicit: node.pointer("/contentRating/label").and_then(Value::as_str)
                            == Some("EXPLICIT"),
                        popularity: None,
                        artists: artist_names(node),
                        artist_ids: artist_ids(node),
                        album_id: node_id(&node["album"], "album").map(|(id, _)| id),
                        album_name: text(&node["album"], "name"),
                        album_artists: artist_names(&node["album"]),
                        cover_url: cover(node),
                        preview_url: None,
                    })
                })
                .take(limit)
                .collect(),
        ),
        SearchType::Album => SearchItems::Albums(
            nodes()
                .filter_map(|node| {
                    let (id, uri) = node_id(node, "album")?;
                    Some(SimpleAlbum {
                        album_id: id,
                        uri,
                        album_name: text(node, "name")?,
                        artists: artist_names(node),
                        artist_ids: artist_ids(node),
                        cover_url: cover(node),
                    })
                })
                .take(limit)
                .collect(),
        ),
        SearchType::Artist => SearchItems::Artists(
            nodes()
                .filter_map(|node| {
                    let (id, uri) = node_id(node, "artist")?;
                    Some(SimpleArtist {
                        artist_id: id,
                        uri,
                        artist_name: text(&node["profile"], "name")
                            .or_else(|| text(node, "name"))?,
                        genres: Vec::new(),
                        popularity: None,
                        cover_url: cover(node),
                    })
                })
                .take(limit)
                .collect(),
        ),
        SearchType::Playlist => SearchItems::Playlists(
            nodes()
                .filter_map(|node| {
                    let (id, uri) = node_id(node, "playlist")?;
                    Some(SimplePlaylist {
                        playlist_id: id,
                        uri,
                        playlist_name: text(node, "name")?,
                        owner: text(&node["ownerV2"]["data"], "name")
                            .or_else(|| text(&node["owner"], "name"))
                            .unwrap_or_default(),
                        total_tracks: node
                            .pointer("/content/totalCount")
                            .and_then(Value::as_u64)
                            .and_then(|total| u32::try_from(total).ok()),
                        cover_url: cover(node),
                    })
                })
                .take(limit)
                .collect(),
        ),
        _ => unreachable!(),
    })
}

pub struct CatalogFallback {
    anonymous: AnonymousSearch,
    session: std::sync::RwLock<Option<Session>>,
    metadata_slots: Semaphore,
    tracks: Mutex<HashMap<String, (Instant, TrackItem)>>,
}

impl std::fmt::Debug for CatalogFallback {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CatalogFallback")
            .field(
                "has_playback_session",
                &self.session.read().unwrap().is_some(),
            )
            .finish_non_exhaustive()
    }
}

impl Default for CatalogFallback {
    fn default() -> Self {
        Self::new(None)
    }
}

impl CatalogFallback {
    pub fn new(session: Option<Session>) -> Self {
        Self {
            anonymous: AnonymousSearch::default(),
            session: std::sync::RwLock::new(session),
            metadata_slots: Semaphore::new(4),
            tracks: Mutex::new(HashMap::new()),
        }
    }

    pub async fn search_raw(
        &self,
        query: &str,
        limit: u32,
        offset: u32,
    ) -> Result<Value, FallbackError> {
        self.anonymous.search(query, limit, offset).await
    }

    #[cfg(test)]
    pub(crate) fn with_anonymous_for_test(anonymous: AnonymousSearch) -> Self {
        Self {
            anonymous,
            ..Self::default()
        }
    }

    pub async fn search(
        &self,
        query: &str,
        category: SearchType,
        limit: u32,
    ) -> Result<CatalogSearch, FallbackError> {
        let response = self.search_raw(query, limit.max(5), 0).await?;
        let search = response
            .pointer("/data/searchV2")
            .filter(|value| value.is_object())
            .or_else(|| response.pointer("/data/search").filter(|value| value.is_object()))
            .ok_or_else(|| FallbackError::protocol("Spotify anonymous search returned no search data; the web-player protocol may have changed"))?;
        let items = anonymous_results(search, category, limit as usize)?;
        if let SearchItems::Tracks(tracks) = &items {
            let mut cache = self.tracks.lock().await;
            if cache.len() >= 2048 {
                cache.clear();
            }
            for track in tracks {
                cache.insert(
                    track.uri.clone(),
                    (
                        Instant::now(),
                        TrackItem {
                            track_id: track.track_id.clone(),
                            track_name: track.track_name.clone(),
                            artists: track.artists.clone(),
                            artist_ids: track.artist_ids.clone(),
                            queue_idx: None,
                            album_id: track.album_id.clone(),
                            album_name: track.album_name.clone(),
                            cover_url: track.cover_url.clone(),
                            playlist_id: None,
                            playlist_name: None,
                        },
                    ),
                );
            }
        }
        println!("[Catalogue/Anonymous] Search succeeded: category={category:?}, limit={limit}.");
        Ok(CatalogSearch {
            items,
            source: "spotify_anonymous",
        })
    }

    pub fn set_session(&self, session: Session) {
        *self.session.write().unwrap() = Some(session);
    }

    fn session(&self) -> Result<Session, String> {
        self.session.read().unwrap().clone().ok_or_else(|| {
            "Playback session is unavailable for catalogue metadata fallback".to_owned()
        })
    }

    async fn track(&self, uri: &SpotifyUri) -> Result<TrackItem, String> {
        let key = uri
            .to_uri()
            .map_err(|e| format!("Invalid track URI: {e}"))?;
        {
            let cache = self.tracks.lock().await;
            if let Some((at, item)) = cache.get(&key) {
                if at.elapsed() < Duration::from_secs(900) {
                    return Ok(item.clone());
                }
            }
        }
        let _permit = self
            .metadata_slots
            .acquire()
            .await
            .map_err(|_| "Metadata fallback is unavailable")?;
        // Recheck after the permit to share a cached result from a preceding request.
        {
            let cache = self.tracks.lock().await;
            if let Some((at, item)) = cache.get(&key) {
                if at.elapsed() < Duration::from_secs(900) {
                    return Ok(item.clone());
                }
            }
        }
        let track =
            tokio::time::timeout(Duration::from_secs(25), Track::get(&self.session()?, uri))
                .await
                .map_err(|_| "Playback-session track metadata timed out".to_owned())?
                .map_err(|e| format!("Playback-session track metadata failed: {e}"))?;
        let item = TrackItem {
            track_id: uri_id(&track.id)?,
            track_name: track.name,
            artists: track.artists.iter().map(|a| a.name.clone()).collect(),
            queue_idx: None,
            artist_ids: track
                .artists
                .iter()
                .filter_map(|a| uri_id(&a.id).ok())
                .collect(),
            album_id: uri_id(&track.album.id).ok(),
            album_name: Some(track.album.name),
            cover_url: track
                .album
                .covers
                .iter()
                .max_by_key(|image| image.height)
                .and_then(|image| image.id.to_base16().ok())
                .map(|id| format!("https://i.scdn.co/image/{id}")),
            playlist_id: None,
            playlist_name: None,
        };
        let mut cache = self.tracks.lock().await;
        if cache.len() >= 2048 {
            cache.retain(|_, (at, _)| at.elapsed() < Duration::from_secs(900));
        }
        if cache.len() >= 2048 {
            cache.clear();
        }
        cache.insert(key, (Instant::now(), item.clone()));
        Ok(item)
    }

    async fn tracks(&self, uris: Vec<SpotifyUri>) -> Result<Vec<TrackItem>, String> {
        let results = stream::iter(
            uris.into_iter()
                .filter(|uri| matches!(uri, SpotifyUri::Track { .. }))
                .map(|uri| async move { self.track(&uri).await }),
        )
        .buffered(4)
        .collect::<Vec<_>>()
        .await;
        results.into_iter().collect()
    }

    async fn album(&self, uri: &SpotifyUri) -> Result<Vec<TrackItem>, String> {
        let album =
            tokio::time::timeout(Duration::from_secs(25), Album::get(&self.session()?, uri))
                .await
                .map_err(|_| "Playback-session album metadata timed out".to_owned())?
                .map_err(|e| format!("Playback-session album metadata failed: {e}"))?;
        self.tracks(album.tracks().cloned().collect()).await
    }

    pub async fn resolve(&self, category: &str, id: &str) -> Result<Vec<TrackItem>, String> {
        let uri = SpotifyUri::from_uri(&format!("spotify:{category}:{id}"))
            .map_err(|e| format!("Invalid {category} id: {e}"))?;
        let result = match category {
            "track" => vec![self.track(&uri).await?],
            "album" => self.album(&uri).await?,
            "artist" => {
                let artist = tokio::time::timeout(
                    Duration::from_secs(25),
                    Artist::get(&self.session()?, &uri),
                )
                .await
                .map_err(|_| "Playback-session artist metadata timed out".to_owned())?
                .map_err(|e| format!("Playback-session artist metadata failed: {e}"))?;
                let mut result = Vec::new();
                let mut seen_albums = HashSet::new();
                let mut seen_tracks = HashSet::new();
                for album in artist.albums_current().chain(artist.singles_current()) {
                    if seen_albums.insert(album.clone()) {
                        for track in self.album(album).await? {
                            if seen_tracks.insert(track.track_id.clone()) {
                                result.push(track);
                            }
                        }
                    }
                }
                result
            }
            "playlist" => {
                let playlist = tokio::time::timeout(
                    Duration::from_secs(25),
                    Playlist::get(&self.session()?, &uri),
                )
                .await
                .map_err(|_| "Playback-session playlist metadata timed out".to_owned())?
                .map_err(|e| format!("Playback-session playlist metadata failed: {e}"))?;
                if playlist.contents.is_truncated
                    || playlist.contents.position != 0
                    || playlist.contents.items.len() < playlist.length.max(0) as usize
                {
                    return Err("Playback-session playlist metadata was incomplete; refusing to return a partial playlist".to_owned());
                }
                let mut tracks = self.tracks(playlist.tracks().cloned().collect()).await?;
                for track in &mut tracks {
                    track.playlist_id = Some(id.to_owned());
                    track.playlist_name = Some(playlist.name().to_owned());
                }
                tracks
            }
            _ => return Err("Invalid category".to_owned()),
        };
        println!(
            "[Catalogue/Playback session] Resolved {} tracks: category={category}, id={id}.",
            result.len()
        );
        Ok(result)
    }
}

fn uri_id(uri: &SpotifyUri) -> Result<String, String> {
    uri.to_uri()
        .map_err(|e| format!("Invalid metadata URI: {e}"))?
        .rsplit(':')
        .next()
        .map(str::to_owned)
        .ok_or_else(|| "Invalid metadata URI".to_owned())
}
