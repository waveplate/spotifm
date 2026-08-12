use serde::{Deserialize, Serialize};
use std::time::Instant;
use tokio::sync::oneshot;

/// A track in your library or queue
#[derive(Clone, Serialize, Deserialize)]
pub struct TrackItem {
    pub track_id: String,
    pub track_name: String,
    pub artists: Vec<String>,
    pub queue_idx: Option<usize>,
    #[serde(default)]
    pub artist_ids: Vec<String>,
    #[serde(default)]
    pub album_id: Option<String>,
    #[serde(default)]
    pub album_name: Option<String>,
    #[serde(default)]
    pub cover_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub playlist_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub playlist_name: Option<String>,
}

/// What you return when someone asks “what’s playing now?” or MUTATES play state (like play/skip)
#[derive(Clone, Serialize, Deserialize)]
pub struct NowPlaying {
    pub status: String,
    pub track_id: Option<String>,
    pub track_name: String,
    pub artists: Vec<String>,
    #[serde(default)]
    pub artist_ids: Vec<String>,
    #[serde(default)]
    pub album_id: Option<String>,
    #[serde(default)]
    pub album_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub track_duration_ms: Option<u32>,
    pub position_ms: Option<u32>,
    pub listeners: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_playlist: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cover_url: Option<String>,
}

#[derive(Clone, Serialize)]
pub struct ActivePlaylistResponse {
    pub name: String,
    pub tracks: Vec<TrackItem>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct PlaylistInfo {
    pub name: String,
    pub num_tracks: usize,
    pub cover_urls: Vec<String>,
    pub artists: Vec<String>,
    pub last_modified: Option<u64>,
}

#[derive(Clone, Serialize)]
pub struct PlaylistAddResult {
    pub added_tracks: usize,
    pub total_tracks: usize,
}

#[derive(Deserialize)]
pub struct AppendQuery {
    pub playlist: Option<String>,
}

#[derive(Deserialize)]
pub struct PlaySearchQuery {
    pub q: String,
    pub playlist: Option<String>,
}

#[derive(Deserialize)]
pub struct PlaylistSortQuery {
    pub by: String,
}

/// All the messages you send into the playback manager
pub enum Command {
    QueueTracks {
        tracks: Vec<TrackItem>,
        resp: oneshot::Sender<NowPlaying>,
    },
    SkipN {
        count: i32,
        resp: oneshot::Sender<NowPlaying>,
    },
    GetNowPlaying {
        resp: oneshot::Sender<NowPlaying>,
    },
    GetPlaylist {
        resp: oneshot::Sender<Vec<TrackItem>>,
    },
    GetActivePlaylist {
        resp: oneshot::Sender<ActivePlaylistResponse>,
    },
    GetQueue {
        resp: oneshot::Sender<Vec<TrackItem>>,
    },
    GetNext {
        resp: oneshot::Sender<Option<NowPlaying>>,
    },
    SwitchPlaylist {
        name: String,
        resp: oneshot::Sender<Result<Vec<TrackItem>, String>>,
    },
    ShufflePlaylist {
        name: Option<String>,
        resp: oneshot::Sender<Result<Vec<TrackItem>, String>>,
    },
    SortPlaylist {
        name: Option<String>,
        by: String,
        resp: oneshot::Sender<Result<Vec<TrackItem>, String>>,
    },
    RemoveTrack {
        track_id: String,
        resp: oneshot::Sender<Result<Vec<TrackItem>, String>>,
    },
    RemoveQueueTrack {
        track_id: String,
        resp: oneshot::Sender<Result<Vec<TrackItem>, String>>,
    },
    PlayPlaylistTrack {
        track_id: String,
        resp: oneshot::Sender<Result<NowPlaying, String>>,
    },
    PlayNowCustom {
        tracks: Vec<TrackItem>,
        playlist_name: Option<String>,
        resp: oneshot::Sender<NowPlaying>,
    },
    AddTracks {
        playlist_name: String,
        tracks: Vec<TrackItem>,
        resp: oneshot::Sender<Result<PlaylistAddResult, String>>,
    },
    DeleteWhere {
        playlist_name: String,
        tracks: Vec<String>,
        albums: Vec<String>,
        artists: Vec<String>,
        playlist_tracks: Vec<String>,
        resp: oneshot::Sender<Result<(), String>>,
    },
    DeletePlaylist {
        playlist_name: String,
        resp: oneshot::Sender<Result<(), String>>,
    },
    SkipCategory {
        category: String,
        resp: oneshot::Sender<NowPlaying>,
    },
    BroadcastNowPlaying,
}

/// Query parameters for `/search`
#[derive(Deserialize)]
pub struct SearchQuery {
    pub q: String,
}

/// Query parameters for the live stream endpoint.
#[derive(Deserialize, Default)]
pub struct StreamQuery {
    #[serde(default)]
    pub sid: Option<String>,
}

/// Query parameters for the lyrics WebSocket endpoint.
#[derive(Deserialize, Default)]
pub struct WsQuery {
    #[serde(default)]
    pub sid: Option<String>,
}

/// Per-stream-session metadata exposed to the player for sync.
#[derive(Clone, Serialize)]
pub struct StreamSessionMeta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_granule_sec: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_origin_sec: Option<f64>,
    #[serde(skip)]
    pub latest_granule_span_start_sec: Option<f64>,
    #[serde(skip)]
    pub latest_granule_at: Option<Instant>,
    #[serde(skip)]
    pub last_seen: Instant,
    #[serde(skip)]
    pub active: bool,
    #[serde(skip)]
    pub connection_count: usize,
}

impl Default for StreamSessionMeta {
    fn default() -> Self {
        Self {
            latest_granule_sec: None,
            stream_origin_sec: None,
            latest_granule_span_start_sec: None,
            latest_granule_at: None,
            last_seen: Instant::now(),
            active: true,
            connection_count: 0,
        }
    }
}

/// The “flat” track you return from `/search`
#[derive(Serialize)]
pub struct ExtendedTrack {
    pub track_id: String,
    pub uri: String,
    pub track_name: String,
    pub duration_ms: i32,
    pub explicit: bool,
    pub popularity: Option<i32>,
    pub artists: Vec<String>,
    #[serde(default)]
    pub artist_ids: Vec<String>,
    #[serde(default)]
    pub album_id: Option<String>,
    pub album_name: Option<String>,
    pub album_artists: Vec<String>,
    pub cover_url: Option<String>,
    pub preview_url: Option<String>,
}

/// The album you return from `/search/album`
#[derive(Serialize, Clone)]
pub struct SimpleAlbum {
    pub album_id: String,
    pub uri: String,
    pub album_name: String,
    pub artists: Vec<String>,
    pub artist_ids: Vec<String>,
    pub cover_url: Option<String>,
}

/// The artist you return from `/search/artist`
#[derive(Serialize, Clone)]
pub struct SimpleArtist {
    pub artist_id: String,
    pub uri: String,
    pub artist_name: String,
    pub genres: Vec<String>,
    pub popularity: Option<i32>,
    pub cover_url: Option<String>,
}

/// The playlist you return from `/search/playlist`
#[derive(Serialize, Clone)]
pub struct SimplePlaylist {
    pub playlist_id: String,
    pub uri: String,
    pub playlist_name: String,
    pub owner: String,
    pub total_tracks: u32,
    pub cover_url: Option<String>,
}

/// WebSocket messages sent to all connected clients
#[derive(Clone, Serialize)]
#[serde(tag = "type")]
pub enum WsMessage {
    Lyrics {
        track_id: String,
        background: u32,
        text_color: u32,
        highlight_color: u32,
        lines: Vec<SimpleLyricLine>,
    },
    Position {
        position_ms: u32,
        #[serde(skip_serializing_if = "Option::is_none")]
        track_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        start_granule_sec: Option<f64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        stream_origin_sec: Option<f64>,
    },
    NoLyrics,
    Idle,
    NowPlaying(NowPlaying),
    Playlist(ActivePlaylistResponse),
    Next {
        #[serde(flatten)]
        next: Option<NowPlaying>,
    },
}

pub type LyricsMessage = WsMessage;

#[derive(Clone, Serialize)]
pub struct SimpleLyricLine {
    pub time_ms: u32,
    pub text: String,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct PlaylistTracksBody {
    #[serde(default)]
    pub tracks: Vec<String>,
    #[serde(default)]
    pub albums: Vec<String>,
    #[serde(default)]
    pub artists: Vec<String>,
    #[serde(default)]
    pub playlists: Vec<String>,
}
