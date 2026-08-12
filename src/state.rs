use crate::models::Command;
use librespot::core::session::Session;
use librespot::playback::config::PlayerConfig;
use rspotify::AuthCodePkceSpotify;
use std::collections::HashMap;
use std::convert::Infallible;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::mpsc::UnboundedSender;
use warp::Filter;

/// All the data you share across handlers + the playback manager
pub struct AppState {
    pub session: std::sync::Mutex<Session>,
    pub player_cfg: PlayerConfig,
    pub spotify: AuthCodePkceSpotify,
    pub spotify_tokens: crate::spotify::SpotifyTokenManager,
    pub cmd_tx: UnboundedSender<Command>,
    pub playlist_file: Option<PathBuf>,
    pub start_time: Arc<Mutex<Option<Instant>>>,
    pub position_ms: Arc<Mutex<Option<u32>>>,
    pub tx_pcm: crate::audio::stream_manager::PcmSender,
    pub tx_mp3: tokio::sync::broadcast::Sender<bytes::Bytes>,
    pub stream_headers: Arc<Mutex<Vec<bytes::Bytes>>>,
    pub playback_timeline: crate::audio::stream_manager::PlaybackTimeline,
    pub ogg_page_index: crate::audio::stream_manager::OggPageIndex,
    pub tx_lyrics_ws: tokio::sync::broadcast::Sender<crate::models::LyricsMessage>,
    pub current_lyrics: Arc<Mutex<Option<crate::models::LyricsMessage>>>,
    pub cli: crate::Cli,
    pub total_samples: Arc<Mutex<u64>>,
    pub stream_sessions: Arc<Mutex<HashMap<String, crate::models::StreamSessionMeta>>>,
}

const ACTIVE_STREAM_SESSION_TTL: Duration = Duration::from_secs(15);

impl AppState {
    pub async fn refresh_spotify_token(&self) -> rspotify::ClientResult<()> {
        self.spotify_tokens.refresh(&self.spotify).await
    }

    pub async fn ensure_spotify_token(&self) -> rspotify::ClientResult<()> {
        self.spotify_tokens.ensure_fresh(&self.spotify).await
    }

    pub fn active_listener_count(&self) -> u32 {
        let now = Instant::now();
        let mut sessions = self.stream_sessions.lock().unwrap();
        sessions.retain(|_, meta| now.duration_since(meta.last_seen) <= ACTIVE_STREAM_SESSION_TTL);
        sessions.values().filter(|meta| meta.active).count() as u32
    }
}

/// A small Warp filter to give every handler a copy of Arc<AppState>
pub fn with_state(
    s: Arc<AppState>,
) -> impl Filter<Extract = (Arc<AppState>,), Error = Infallible> + Clone {
    warp::any().map(move || s.clone())
}
