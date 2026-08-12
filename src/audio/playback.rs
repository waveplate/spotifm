use crate::models::{ActivePlaylistResponse, Command, NowPlaying, PlaylistAddResult, TrackItem};
use crate::state::AppState;
use crate::Cli;
use librespot::core::spotify_id::SpotifyId;
use librespot::core::SpotifyUri;
use librespot::playback::audio_backend::Sink;
use librespot::playback::mixer::NoOpVolume;
use librespot::playback::player::{Player, PlayerEvent};

use std::{
    fs::{File, OpenOptions},
    io::{BufReader, Write},
    path::PathBuf,
    sync::Arc,
    time::Instant,
};
use tokio::sync::mpsc::UnboundedReceiver;

/// Save the in-memory playlist back to disk (if a file was given).
fn save_playlist(pl: &[TrackItem], playlist_file: &Option<PathBuf>) {
    if let Some(path) = playlist_file {
        if let Err(e) = (|| -> std::io::Result<()> {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut f = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(path)?;
            let data = serde_json::to_vec_pretty(pl)?;
            f.write_all(&data)?;
            println!("[Playback] Saved {} tracks to {}", pl.len(), path.display());
            Ok(())
        })() {
            eprintln!("[Playback] ⚠️ Could not save to {}: {}", path.display(), e);
        }
    }
}

/// Pick the next track (queued first, then library in round-robin).
fn select_next(pl: &[TrackItem], idx: usize) -> Option<(usize, TrackItem)> {
    let mut queued: Vec<(usize, &TrackItem)> = pl
        .iter()
        .enumerate()
        .filter(|(_, i)| i.queue_idx.is_some())
        .collect();
    queued.sort_by_key(|(_, i)| i.queue_idx.unwrap());
    if let Some((pos, item)) = queued.first() {
        println!("[Playback] Next from queue: '{}'", item.track_name);
        return Some((*pos, (*item).clone()));
    }
    let lib: Vec<(usize, &TrackItem)> = pl
        .iter()
        .enumerate()
        .filter(|(_, i)| i.queue_idx.is_none())
        .collect();
    if lib.is_empty() {
        return None;
    }
    let idx_mod = idx % lib.len();
    let (pos, item) = lib[idx_mod];
    println!(
        "[Playback] Next from library #{}: '{}'",
        idx_mod, item.track_name
    );
    Some((pos, item.clone()))
}

pub(crate) fn get_playlist_dir(default_path: &Option<PathBuf>) -> PathBuf {
    let base_dir = if let Some(path) = default_path {
        if path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
        {
            path.parent()
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."))
        } else {
            path.clone()
        }
    } else {
        PathBuf::from("playlists")
    };

    let _ = std::fs::create_dir_all(&base_dir);
    base_dir
}

/// Helper function to resolve the target playlist file path by name.
pub(crate) fn get_playlist_path(name: &str, default_path: &Option<PathBuf>) -> PathBuf {
    if name.eq_ignore_ascii_case("default") {
        if let Some(path) = default_path {
            return path.clone();
        }
    }

    let base_dir = get_playlist_dir(default_path);
    base_dir.join(format!("{}.json", name))
}

pub(crate) fn list_playlist_names(default_path: &Option<PathBuf>) -> Result<Vec<String>, String> {
    let playlist_dir = get_playlist_dir(default_path);
    let default_path_canonical = default_path
        .as_ref()
        .map(|path| path.canonicalize().unwrap_or_else(|_| path.clone()));

    let mut names = std::collections::BTreeSet::new();
    names.insert("default".to_string());

    let entries = std::fs::read_dir(&playlist_dir).map_err(|e| {
        format!(
            "Failed to list playlists in {}: {}",
            playlist_dir.display(),
            e
        )
    })?;

    for entry in entries {
        let entry = entry.map_err(|e| format!("Failed to read playlist directory entry: {}", e))?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }

        let canonical_path = path.canonicalize().unwrap_or_else(|_| path.clone());
        if default_path_canonical
            .as_ref()
            .is_some_and(|default_path| *default_path == canonical_path)
        {
            names.insert("default".to_string());
            continue;
        }

        if let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) {
            names.insert(stem.to_string());
        }
    }

    Ok(names.into_iter().collect())
}

/// Helper function to get the current active playlist name.
fn get_active_playlist_name(
    current_path: &Option<PathBuf>,
    default_path: &Option<PathBuf>,
) -> String {
    if let Some(path) = current_path {
        if let Some(gp) = default_path {
            if path == gp {
                "default".to_string()
            } else {
                path.file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("default")
                    .to_string()
            }
        } else {
            path.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("default")
                .to_string()
        }
    } else {
        "default".to_string()
    }
}

fn build_now_playing(
    track: Option<&TrackItem>,
    status: &str,
    position_ms: Option<u32>,
    track_duration_ms: Option<u32>,
    listeners: u32,
    active_playlist: String,
) -> NowPlaying {
    if let Some(track) = track {
        NowPlaying {
            status: status.to_string(),
            track_id: Some(track.track_id.clone()),
            track_name: track.track_name.clone(),
            artists: track.artists.clone(),
            artist_ids: track.artist_ids.clone(),
            album_id: track.album_id.clone(),
            album_name: track.album_name.clone(),
            track_duration_ms,
            position_ms,
            listeners,
            active_playlist: Some(active_playlist),
            cover_url: track.cover_url.clone(),
        }
    } else {
        NowPlaying {
            status: status.to_string(),
            track_id: None,
            track_name: "".into(),
            artists: Vec::new(),
            artist_ids: Vec::new(),
            album_id: None,
            album_name: None,
            track_duration_ms: None,
            position_ms,
            listeners,
            active_playlist: Some(active_playlist),
            cover_url: None,
        }
    }
}

fn get_sorted_queue(playlist: &[TrackItem]) -> Vec<TrackItem> {
    let mut queue: Vec<TrackItem> = playlist
        .iter()
        .filter(|item| item.queue_idx.is_some())
        .cloned()
        .collect();
    queue.sort_by_key(|item| item.queue_idx.unwrap_or(usize::MAX));
    queue
}

fn sort_string(value: &str) -> String {
    value.to_ascii_lowercase()
}

fn sort_optional_string(value: Option<&str>) -> String {
    value.map(sort_string).unwrap_or_default()
}

fn primary_artist_sort_key(track: &TrackItem) -> String {
    sort_optional_string(track.artists.first().map(String::as_str))
}

fn sort_playlist_tracks(playlist: &mut [TrackItem], by: &str) -> Result<(), String> {
    match by {
        "artist" => {
            playlist.sort_by_key(|track| {
                (
                    primary_artist_sort_key(track),
                    sort_optional_string(track.album_name.as_deref()),
                    sort_string(&track.track_name),
                    track.track_id.clone(),
                )
            });
        }
        "album" => {
            playlist.sort_by_key(|track| {
                (
                    track.album_name.is_none(),
                    sort_optional_string(track.album_name.as_deref()),
                    sort_string(&track.track_name),
                    primary_artist_sort_key(track),
                    track.track_id.clone(),
                )
            });
        }
        "playlist" => {
            playlist.sort_by_key(|track| {
                (
                    track.playlist_name.is_none(),
                    sort_optional_string(track.playlist_name.as_deref()),
                    sort_optional_string(track.playlist_id.as_deref()),
                    primary_artist_sort_key(track),
                    sort_optional_string(track.album_name.as_deref()),
                    sort_string(&track.track_name),
                    track.track_id.clone(),
                )
            });
        }
        _ => {
            return Err(format!(
                "Invalid sort key '{}'. Expected artist, album, or playlist",
                by
            ));
        }
    }

    Ok(())
}

fn queue_tracks(playlist: &mut Vec<TrackItem>, tracks: &[TrackItem]) -> (bool, bool) {
    let queue_was_empty = !playlist.iter().any(|item| item.queue_idx.is_some());
    let mut next_idx = playlist
        .iter()
        .filter_map(|item| item.queue_idx)
        .max()
        .map(|idx| idx + 1)
        .unwrap_or(0);
    let mut changed = false;

    for item in tracks {
        if let Some(existing) = playlist
            .iter_mut()
            .find(|playlist_item| playlist_item.track_id == item.track_id)
        {
            if existing.queue_idx.is_none() {
                existing.queue_idx = Some(next_idx);
                next_idx += 1;
                changed = true;
            }
        } else {
            let mut queued = item.clone();
            queued.queue_idx = Some(next_idx);
            playlist.push(queued);
            next_idx += 1;
            changed = true;
        }
    }

    (changed, queue_was_empty)
}

fn consume_selected_track(
    playlist: &mut [TrackItem],
    playlist_idx: &mut usize,
    pos: usize,
) -> bool {
    if playlist[pos].queue_idx.is_some() {
        playlist[pos].queue_idx = None;
        true
    } else {
        let lib_count = playlist
            .iter()
            .filter(|item| item.queue_idx.is_none())
            .count();
        if lib_count > 0 {
            *playlist_idx = (*playlist_idx + 1) % lib_count;
        }
        false
    }
}

fn select_previous_library(
    playlist: &[TrackItem],
    current: Option<&TrackItem>,
    playlist_idx: usize,
    steps: usize,
) -> Option<(usize, usize, TrackItem)> {
    let lib: Vec<(usize, &TrackItem)> = playlist
        .iter()
        .enumerate()
        .filter(|(_, item)| item.queue_idx.is_none())
        .collect();
    if lib.is_empty() {
        return None;
    }

    let len = lib.len();
    let base_idx = current
        .and_then(|cur| {
            lib.iter()
                .position(|(_, item)| item.track_id == cur.track_id)
        })
        .unwrap_or_else(|| playlist_idx % len);
    let target_idx = (base_idx + len - (steps % len)) % len;
    let (pos, item) = lib[target_idx];

    println!(
        "[Playback] Previous from library #{}: '{}'",
        target_idx, item.track_name
    );

    Some((pos, (target_idx + 1) % len, item.clone()))
}

fn new_streaming_sink(state: &Arc<AppState>) -> Box<dyn Sink> {
    if state.player_cfg.passthrough {
        Box::new(
            crate::audio::stream_manager::PassthroughOggStreamingSink::new(
                state.tx_mp3.clone(),
                state.stream_headers.clone(),
                state.playback_timeline.clone(),
                state.ogg_page_index.clone(),
                state.total_samples.clone(),
            ),
        )
    } else {
        Box::new(crate::audio::stream_manager::GstreamerStreamingSink::new(
            state.tx_pcm.clone(),
            state.playback_timeline.clone(),
        ))
    }
}

/// This task drives the actual LibreSpot player and reacts to commands & end-of-track.
pub async fn playback_manager(
    mut cmd_rx: UnboundedReceiver<Command>,
    cli: Cli,
    state: Arc<AppState>,
) {
    if state.player_cfg.passthrough {
        println!("[Playback] Initializing with Ogg/Vorbis passthrough streaming backend.");
    } else {
        println!("[Playback] Initializing with GStreamer streaming backend.");
    }
    let state_for_sink = state.clone();
    let mut player = Player::new(
        state.player_cfg.clone(),
        state.session.lock().unwrap().clone(),
        Box::new(NoOpVolume),
        move || new_streaming_sink(&state_for_sink),
    );

    // ——— Fix: open two independent event channels ———
    let mut ev_rx = player.get_player_event_channel();

    println!("[Playback] Player ready.");

    let mut current_playlist_path = state.playlist_file.clone();
    let default_playlist_path = state.playlist_file.clone();

    // load from disk
    let mut playlist: Vec<TrackItem> = Vec::new();
    if let Some(path) = &current_playlist_path {
        if let Ok(f) = File::open(path) {
            if let Ok(meta) = f.metadata() {
                if meta.len() > 0 {
                    let reader = BufReader::new(f);
                    if let Ok(pl) = serde_json::from_reader::<_, Vec<TrackItem>>(reader) {
                        playlist = pl;
                        println!("[Playback] Loaded {} tracks.", playlist.len());
                    }
                }
            }
        }
    }

    let mut playlist_idx = 0;
    let mut current: Option<TrackItem> = None;
    let mut next_preload: Option<SpotifyUri> = None;

    // **New**: track consecutive unavailable tracks to detect session corruption or outages
    let mut consecutive_unavailable_count = 0;

    // **New**: store the duration of the current track (in ms)
    let mut current_track_duration_ms: Option<u32> = None;

    // **New**: high-precision WebSocket synced lyrics variables
    let mut position_ms_offset: u32 = 0;
    let mut start_instant: Option<Instant> = None;
    let mut track_start_samples: Option<u64> = None;
    let mut position_track_id: Option<String> = None;
    let mut interval = tokio::time::interval(std::time::Duration::from_millis(cli.lyrics_interval));

    // initial play
    if !playlist.is_empty() {
        if let Some((pos, item)) = select_next(&playlist, playlist_idx) {
            if let Ok(sid) = SpotifyId::from_base62(&item.track_id) {
                println!("[Playback] Starting '{}'", item.track_name);
                state
                    .playback_timeline
                    .mark_next_buffer_as_track_start(item.track_id.clone());
                player.load(SpotifyUri::Track { id: sid }, true, 0);
                player.play();
                current = Some(item.clone());
                if playlist[pos].queue_idx.is_none() {
                    playlist_idx = (playlist_idx + 1)
                        % playlist.iter().filter(|i| i.queue_idx.is_none()).count();
                }
            }
        }
    }

    loop {
        // preload next
        if next_preload.is_none() {
            if let Some((_, ni)) = select_next(&playlist, playlist_idx) {
                if let Ok(sid) = SpotifyId::from_base62(&ni.track_id) {
                    let uri = SpotifyUri::Track { id: sid };
                    println!("[Playback] Preloading '{}'", ni.track_name);
                    player.preload(uri.clone());
                    next_preload = Some(uri);
                }
            }
        }

        tokio::select! {
            // ——— High-precision WebSocket Sync Ticks ———
            _ = interval.tick() => {
                if let Some(start_time) = start_instant {
                    let elapsed = start_time.elapsed().as_millis() as u32;
                    let current_pos = position_ms_offset + elapsed;
                    let timeline_track_start = state.playback_timeline.current_track_start()
                        .and_then(|track_start| {
                            if position_track_id.as_deref() == Some(track_start.track_id.as_str()) {
                                Some(track_start.stream_sec)
                            } else {
                                None
                            }
                        });
                    let start_granule_sec = timeline_track_start.or_else(|| {
                        track_start_samples.map(|samples| samples as f64 / 44100.0)
                    });
                    let _ = state.tx_lyrics_ws.send(crate::models::LyricsMessage::Position {
                        position_ms: current_pos,
                        track_id: position_track_id.clone(),
                        start_granule_sec,
                        stream_origin_sec: None,
                    });
                }
            }

            // ——— Command handling ———
            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(Command::QueueTracks { tracks, resp }) => {
                        println!("[CMD] Queue {} track(s)", tracks.len());
                        if let Some(first) = tracks.first().cloned() {
                            let (changed, queue_was_empty) = queue_tracks(&mut playlist, &tracks);
                            if changed {
                                save_playlist(&playlist, &current_playlist_path);
                            }
                            if changed && queue_was_empty { next_preload = None; }
                            let _ = resp.send(build_now_playing(
                                Some(&first),
                                "queued",
                                None,
                                None,
                                state.active_listener_count(),
                                get_active_playlist_name(&current_playlist_path, &default_playlist_path),
                            ));
                        } else {
                            let _ = resp.send(build_now_playing(
                                current.as_ref(),
                                if current.is_some() { "playing" } else { "idle" },
                                None,
                                current_track_duration_ms,
                                state.active_listener_count(),
                                get_active_playlist_name(&current_playlist_path, &default_playlist_path),
                            ));
                        }
                    }

                    Some(Command::SkipN { count, resp }) => {
                        println!("[CMD] SkipN {count}");
                        player.stop();

                        let mut final_item = None;
                        let mut queue_changed = false;

                        if count > 0 {
                            for _ in 0..count {
                                if let Some((pos, item)) = select_next(&playlist, playlist_idx) {
                                    queue_changed |= consume_selected_track(
                                        &mut playlist,
                                        &mut playlist_idx,
                                        pos,
                                    );
                                    final_item = Some(item);
                                } else {
                                    final_item = None;
                                    break;
                                }
                            }
                        } else if let Some((_, next_idx, item)) = select_previous_library(
                            &playlist,
                            current.as_ref(),
                            playlist_idx,
                            count.unsigned_abs() as usize,
                        ) {
                            playlist_idx = next_idx;
                            final_item = Some(item);
                        }

                        if queue_changed {
                            save_playlist(&playlist, &current_playlist_path);
                        }

                        if let Some(item) = final_item {
                            let sid = SpotifyId::from_base62(&item.track_id).unwrap();
                            let uri = SpotifyUri::Track { id: sid };
                            println!("[Playback] Skipping directly to '{}'", item.track_name);
                            current = Some(item.clone());
                            state
                                .playback_timeline
                                .mark_next_buffer_after_restart_as_track_start(
                                    item.track_id.clone(),
                                );
                            player.load(uri, true, 0);
                            player.play();
                            next_preload = None;

                            let _ = resp.send(build_now_playing(
                                Some(&item),
                                "playing",
                                Some(0),
                                None,
                                state.active_listener_count(),
                                get_active_playlist_name(
                                    &current_playlist_path,
                                    &default_playlist_path,
                                ),
                            ));
                        } else {
                            current = None;
                            *state.start_time.lock().unwrap() = None;
                            *state.position_ms.lock().unwrap() = None;
                            current_track_duration_ms = None;
                            position_ms_offset = 0;
                            start_instant = None;
                            track_start_samples = None;
                            position_track_id = None;
                            state.playback_timeline.clear();
                            next_preload = None;

                            let _ = resp.send(build_now_playing(
                                None,
                                "idle",
                                None,
                                None,
                                state.active_listener_count(),
                                get_active_playlist_name(
                                    &current_playlist_path,
                                    &default_playlist_path,
                                ),
                            ));
                        }
                    }

                    Some(Command::GetNowPlaying { resp }) => {
                        // Build the NowPlaying with duration from last TrackChanged
                        let np = build_now_playing(
                            current.as_ref(),
                            if current.is_some() { "playing" } else { "idle" },
                            None,
                            current_track_duration_ms,
                            state.active_listener_count(),
                            get_active_playlist_name(&current_playlist_path, &default_playlist_path),
                        );
                        let _ = resp.send(np);
                    }
                    Some(Command::BroadcastNowPlaying) => {
                        let elapsed = start_instant
                            .map(|start| start.elapsed().as_millis() as u32)
                            .unwrap_or(0);
                        let position_ms = if current.is_some() {
                            Some(position_ms_offset + elapsed)
                        } else {
                            None
                        };
                        let np = build_now_playing(
                            current.as_ref(),
                            if current.is_some() { "playing" } else { "idle" },
                            position_ms,
                            current_track_duration_ms,
                            state.active_listener_count(),
                            get_active_playlist_name(&current_playlist_path, &default_playlist_path),
                        );
                        let _ = state.tx_lyrics_ws.send(crate::models::WsMessage::NowPlaying(np));
                    }
                    Some(Command::GetPlaylist { resp }) => {
                        let _ = resp.send(playlist.clone());
                    }
                    Some(Command::GetActivePlaylist { resp }) => {
                        let active_name =
                            get_active_playlist_name(&current_playlist_path, &default_playlist_path);
                        let _ = resp.send(ActivePlaylistResponse {
                            name: active_name,
                            tracks: playlist.clone(),
                        });
                    }
                    Some(Command::GetQueue { resp }) => {
                        let _ = resp.send(get_sorted_queue(&playlist));
                    }
                    Some(Command::GetNext { resp }) => {
                        let nxt = select_next(&playlist, playlist_idx).map(|(_, item)| {
                            build_now_playing(
                                Some(&item),
                                "next",
                                None,
                                None,
                                state.active_listener_count(),
                                get_active_playlist_name(
                                    &current_playlist_path,
                                    &default_playlist_path,
                                ),
                            )
                        });
                        let _ = resp.send(nxt);
                    }
                    Some(Command::RemoveQueueTrack { track_id, resp }) => {
                        println!("[CMD] Remove from queue: {}", track_id);
                        let mut found = false;
                        if let Some(pos) = playlist.iter().position(|i| i.track_id == track_id && i.queue_idx.is_some()) {
                            playlist[pos].queue_idx = None;
                            save_playlist(&playlist, &current_playlist_path);
                            found = true;
                        }

                        if found {
                            let _ = resp.send(Ok(get_sorted_queue(&playlist)));
                        } else {
                            let _ = resp.send(Err("Track is not in the active transient queue".to_string()));
                        }
                    }
                    Some(Command::PlayPlaylistTrack { track_id, resp }) => {
                        println!("[CMD] Play active playlist track: {track_id}");

                        let Some(pos) = playlist.iter().position(|item| item.track_id == track_id) else {
                            let _ = resp.send(Err("Track is not in the active playlist".to_string()));
                            continue;
                        };
                        let item = playlist[pos].clone();
                        let Ok(sid) = SpotifyId::from_base62(&item.track_id) else {
                            let _ = resp.send(Err("Track has an invalid Spotify ID".to_string()));
                            continue;
                        };

                        player.stop();
                        if item.queue_idx.is_none() {
                            // consume_selected_track advances from playlist_idx. Align it
                            // with the explicitly selected library track so automatic
                            // playback continues with the track that follows this one.
                            playlist_idx = playlist
                                .iter()
                                .take(pos)
                                .filter(|item| item.queue_idx.is_none())
                                .count();
                        }
                        if consume_selected_track(&mut playlist, &mut playlist_idx, pos) {
                            save_playlist(&playlist, &current_playlist_path);
                        }

                        current = Some(item.clone());
                        current_track_duration_ms = None;
                        position_ms_offset = 0;
                        start_instant = None;
                        track_start_samples = None;
                        position_track_id = None;
                        *state.start_time.lock().unwrap() = None;
                        *state.position_ms.lock().unwrap() = Some(0);
                        state
                            .playback_timeline
                            .mark_next_buffer_after_restart_as_track_start(item.track_id.clone());

                        player.load(SpotifyUri::Track { id: sid }, true, 0);
                        player.play();
                        next_preload = None;

                        let _ = resp.send(Ok(build_now_playing(
                            Some(&item),
                            "playing",
                            Some(0),
                            None,
                            state.active_listener_count(),
                            get_active_playlist_name(
                                &current_playlist_path,
                                &default_playlist_path,
                            ),
                        )));
                    }
                    Some(Command::SwitchPlaylist { name, resp }) => {
                        // 1) Save current playlist first
                        save_playlist(&playlist, &current_playlist_path);

                        // 2) Load new playlist
                        let path = get_playlist_path(&name, &default_playlist_path);
                        if !path.exists() {
                            let empty_playlist: Vec<TrackItem> = Vec::new();
                            save_playlist(&empty_playlist, &Some(path.clone()));
                            playlist = empty_playlist;
                            current_playlist_path = Some(path.clone());
                            playlist_idx = 0;
                            next_preload = None;
                            println!(
                                "[Playback] Created and switched to playlist '{}' at {}",
                                name,
                                path.display()
                            );
                            let _ = resp.send(Ok(playlist.clone()));
                        } else {
                            match std::fs::read_to_string(&path) {
                                Ok(content) => {
                                    match serde_json::from_str::<Vec<TrackItem>>(&content) {
                                        Ok(new_pl) => {
                                            playlist = new_pl;
                                            current_playlist_path = Some(path.clone());
                                            playlist_idx = 0;
                                            next_preload = None; // Reset preload since playlist changed
                                            println!("[Playback] Switched to playlist '{}' at {} with {} tracks", name, path.display(), playlist.len());
                                            let _ = resp.send(Ok(playlist.clone()));
                                        }
                                        Err(e) => {
                                            eprintln!("[Playback] Failed to deserialize playlist JSON: {}", e);
                                            let _ = resp.send(Err(format!("Invalid playlist file: {}", e)));
                                        }
                                    }
                                }
                                Err(e) => {
                                    eprintln!("[Playback] Failed to read playlist file: {}", e);
                                    let _ = resp.send(Err(format!("Failed to read playlist file: {}", e)));
                                }
                            }
                        }
                    }
                    Some(Command::ShufflePlaylist { name, resp }) => {
                        use rand::seq::SliceRandom;
                        let mut rng = rand::thread_rng();

                        if let Some(target_name) = name {
                            let path = get_playlist_path(&target_name, &default_playlist_path);
                            let is_current = if let Some(ref curr_path) = current_playlist_path {
                                path == *curr_path
                            } else {
                                false
                            };

                            if is_current {
                                playlist.shuffle(&mut rng);
                                save_playlist(&playlist, &current_playlist_path);
                                let _ = resp.send(Ok(playlist.clone()));
                            } else if !path.exists() {
                                let empty_playlist: Vec<TrackItem> = Vec::new();
                                save_playlist(&empty_playlist, &Some(path.clone()));
                                println!(
                                    "[Playback] Created playlist '{}' at {} during shuffle",
                                    target_name,
                                    path.display()
                                );
                                let _ = resp.send(Ok(empty_playlist));
                            } else {
                                match std::fs::read_to_string(&path) {
                                    Ok(content) => {
                                        match serde_json::from_str::<Vec<TrackItem>>(&content) {
                                            Ok(mut target_pl) => {
                                                target_pl.shuffle(&mut rng);
                                                save_playlist(&target_pl, &Some(path.clone()));
                                                let _ = resp.send(Ok(target_pl));
                                            }
                                            Err(e) => {
                                                let _ = resp.send(Err(format!(
                                                    "Invalid playlist file: {}",
                                                    e
                                                )));
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        let _ = resp
                                            .send(Err(format!("Failed to read playlist file: {}", e)));
                                    }
                                }
                            }
                        } else {
                            playlist.shuffle(&mut rng);
                            save_playlist(&playlist, &current_playlist_path);
                            let _ = resp.send(Ok(playlist.clone()));
                        }
                    }
                    Some(Command::SortPlaylist { name, by, resp }) => {
                        if let Some(target_name) = name {
                            let path = get_playlist_path(&target_name, &default_playlist_path);
                            let is_current = if let Some(ref curr_path) = current_playlist_path {
                                path == *curr_path
                            } else {
                                false
                            };

                            if is_current {
                                match sort_playlist_tracks(&mut playlist, &by) {
                                    Ok(()) => {
                                        next_preload = None;
                                        save_playlist(&playlist, &current_playlist_path);
                                        let _ = resp.send(Ok(playlist.clone()));
                                    }
                                    Err(e) => {
                                        let _ = resp.send(Err(e));
                                    }
                                }
                            } else if !path.exists() {
                                let mut empty_playlist: Vec<TrackItem> = Vec::new();
                                match sort_playlist_tracks(&mut empty_playlist, &by) {
                                    Ok(()) => {
                                        save_playlist(&empty_playlist, &Some(path.clone()));
                                        println!(
                                            "[Playback] Created playlist '{}' at {} during sort",
                                            target_name,
                                            path.display()
                                        );
                                        let _ = resp.send(Ok(empty_playlist));
                                    }
                                    Err(e) => {
                                        let _ = resp.send(Err(e));
                                    }
                                }
                            } else {
                                match std::fs::read_to_string(&path) {
                                    Ok(content) => {
                                        match serde_json::from_str::<Vec<TrackItem>>(&content) {
                                            Ok(mut target_pl) => {
                                                match sort_playlist_tracks(&mut target_pl, &by) {
                                                    Ok(()) => {
                                                        save_playlist(
                                                            &target_pl,
                                                            &Some(path.clone()),
                                                        );
                                                        let _ = resp.send(Ok(target_pl));
                                                    }
                                                    Err(e) => {
                                                        let _ = resp.send(Err(e));
                                                    }
                                                }
                                            }
                                            Err(e) => {
                                                let _ = resp.send(Err(format!(
                                                    "Invalid playlist file: {}",
                                                    e
                                                )));
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        let _ = resp
                                            .send(Err(format!("Failed to read playlist file: {}", e)));
                                    }
                                }
                            }
                        } else {
                            match sort_playlist_tracks(&mut playlist, &by) {
                                Ok(()) => {
                                    next_preload = None;
                                    save_playlist(&playlist, &current_playlist_path);
                                    let _ = resp.send(Ok(playlist.clone()));
                                }
                                Err(e) => {
                                    let _ = resp.send(Err(e));
                                }
                            }
                        }
                    }
                    Some(Command::RemoveTrack { track_id, resp }) => {
                        let old_len = playlist.len();
                        playlist.retain(|t| t.track_id != track_id);
                        let new_len = playlist.len();
                        if old_len == new_len {
                            let _ = resp.send(Err(format!("Track '{}' not found in playlist", track_id)));
                        } else {
                            save_playlist(&playlist, &current_playlist_path);
                            next_preload = None; // Reset preload in case the deleted track was preloaded
                            println!("[Playback] Removed track '{}' from playlist", track_id);
                            let _ = resp.send(Ok(playlist.clone()));
                        }
                    }
                    Some(Command::PlayNowCustom { tracks, playlist_name, resp }) => {
                        if let Some(name) = playlist_name {
                            println!("[CMD] PlayNowCustom on playlist '{}'", name);
                            // 1) Save current playlist first
                            save_playlist(&playlist, &current_playlist_path);

                            // 2) Resolve and load target playlist
                            let target_path = get_playlist_path(&name, &default_playlist_path);
                            let mut target_playlist = Vec::new();
                            if target_path.exists() {
                                if let Ok(f) = File::open(&target_path) {
                                    let reader = BufReader::new(f);
                                    if let Ok(pl) = serde_json::from_reader::<_, Vec<TrackItem>>(reader) {
                                        target_playlist = pl;
                                    }
                                }
                            }

                            // 3) Deduplicate incoming tracks from target playlist
                            let incoming_ids: std::collections::HashSet<String> = tracks.iter().map(|t| t.track_id.clone()).collect();
                            target_playlist.retain(|t| !incoming_ids.contains(&t.track_id));

                            // 4) Append new tracks to the end
                            for t in &tracks {
                                let mut t_clone = t.clone();
                                t_clone.queue_idx = None;
                                target_playlist.push(t_clone);
                            }

                            // 5) Save target playlist
                            save_playlist(&target_playlist, &Some(target_path.clone()));

                            // 6) Switch active in-memory playlist
                            playlist = target_playlist;
                            current_playlist_path = Some(target_path);

                            // 7) Stop and play first new track
                            player.stop();
                            let first_new_track = tracks[0].clone();
                            current = Some(first_new_track.clone());

                            let lib_tracks: Vec<usize> = playlist.iter().enumerate().filter(|(_, t)| t.queue_idx.is_none()).map(|(idx, _)| idx).collect();
                            if let Some(pos_in_lib) = lib_tracks.iter().position(|&idx| playlist[idx].track_id == first_new_track.track_id) {
                                playlist_idx = (pos_in_lib + 1) % lib_tracks.len();
                            } else {
                                playlist_idx = 0;
                            }

                            let sid = SpotifyId::from_base62(&first_new_track.track_id).unwrap();
                            let uri = SpotifyUri::Track { id: sid };
                            state
                                .playback_timeline
                                .mark_next_buffer_after_restart_as_track_start(
                                    first_new_track.track_id.clone(),
                                );
                            player.load(uri, true, 0);
                            player.play();
                            next_preload = None;

                            let _ = resp.send(build_now_playing(
                                Some(&first_new_track),
                                "playing",
                                Some(0),
                                None,
                                state.active_listener_count(),
                                get_active_playlist_name(
                                    &current_playlist_path,
                                    &default_playlist_path,
                                ),
                            ));
                        } else {
                            println!("[CMD] PlayNowCustom on default playlist");
                            // 1) Save current playlist if we are switching away from it
                            let default_path = default_playlist_path
                                .clone()
                                .unwrap_or(PathBuf::from("default.json"));
                            let is_on_default = current_playlist_path.as_ref() == Some(&default_path);

                            if !is_on_default {
                                save_playlist(&playlist, &current_playlist_path);
                                current_playlist_path = Some(default_path.clone());
                                playlist = Vec::new();
                                if default_path.exists() {
                                    if let Ok(f) = File::open(&default_path) {
                                        let reader = BufReader::new(f);
                                        if let Ok(pl) = serde_json::from_reader::<_, Vec<TrackItem>>(reader) {
                                            playlist = pl;
                                        }
                                    }
                                }
                            }

                            // 2) Deduplicate incoming tracks from default playlist
                            let incoming_ids: std::collections::HashSet<String> = tracks.iter().map(|t| t.track_id.clone()).collect();
                            playlist.retain(|t| !incoming_ids.contains(&t.track_id));

                            // 3) Find position of current playing track and insert new tracks immediately after it
                            let mut insert_pos = 0;
                            if let Some(ref cur) = current {
                                if let Some(pos) = playlist.iter().position(|t| t.track_id == cur.track_id) {
                                    insert_pos = pos + 1;
                                } else {
                                    let mut c = cur.clone();
                                    c.queue_idx = None;
                                    playlist.insert(0, c);
                                    insert_pos = 1;
                                }
                            }

                            for (i, t) in tracks.iter().enumerate() {
                                let mut t_clone = t.clone();
                                t_clone.queue_idx = None;
                                playlist.insert(insert_pos + i, t_clone);
                            }

                            // 4) Save updated default playlist
                            save_playlist(&playlist, &current_playlist_path);

                            // 5) Stop and play first new track
                            player.stop();
                            let first_new_track = tracks[0].clone();
                            current = Some(first_new_track.clone());

                            let lib_tracks: Vec<usize> = playlist.iter().enumerate().filter(|(_, t)| t.queue_idx.is_none()).map(|(idx, _)| idx).collect();
                            if let Some(pos_in_lib) = lib_tracks.iter().position(|&idx| playlist[idx].track_id == first_new_track.track_id) {
                                playlist_idx = (pos_in_lib + 1) % lib_tracks.len();
                            } else {
                                playlist_idx = 0;
                            }

                            let sid = SpotifyId::from_base62(&first_new_track.track_id).unwrap();
                            let uri = SpotifyUri::Track { id: sid };
                            state
                                .playback_timeline
                                .mark_next_buffer_after_restart_as_track_start(
                                    first_new_track.track_id.clone(),
                                );
                            player.load(uri, true, 0);
                            player.play();
                            next_preload = None;

                            let _ = resp.send(build_now_playing(
                                Some(&first_new_track),
                                "playing",
                                Some(0),
                                None,
                                state.active_listener_count(),
                                get_active_playlist_name(
                                    &current_playlist_path,
                                    &default_playlist_path,
                                ),
                            ));
                        }
                    }
                    Some(Command::AddTracks { playlist_name, tracks, resp }) => {
                        println!("[CMD] AddTracks to playlist '{}'", playlist_name);
                        let target_path = get_playlist_path(&playlist_name, &default_playlist_path);
                        let mut target_playlist = Vec::new();
                        if target_path.exists() {
                            if let Ok(f) = File::open(&target_path) {
                                let reader = BufReader::new(f);
                                if let Ok(pl) = serde_json::from_reader::<_, Vec<TrackItem>>(reader) {
                                    target_playlist = pl;
                                }
                            }
                        }

                        let original_len = target_playlist.len();
                        let incoming_ids: std::collections::HashSet<String> = tracks.iter().map(|t| t.track_id.clone()).collect();
                        target_playlist.retain(|t| !incoming_ids.contains(&t.track_id));

                        for t in tracks {
                            let mut t_clone = t.clone();
                            t_clone.queue_idx = None;
                            target_playlist.push(t_clone);
                        }

                        let total_tracks = target_playlist.len();
                        save_playlist(&target_playlist, &Some(target_path.clone()));

                        let is_active = if let Some(ref curr_path) = current_playlist_path {
                            target_path == *curr_path
                        } else {
                            false
                        };
                        if is_active {
                            playlist = target_playlist;
                        }

                        let added_tracks = total_tracks.saturating_sub(original_len);

                        let _ = resp.send(Ok(PlaylistAddResult {
                            added_tracks,
                            total_tracks,
                        }));
                    }
                    Some(Command::DeleteWhere { playlist_name, tracks, albums, artists, playlist_tracks, resp }) => {
                        println!("[CMD] DeleteWhere in playlist '{}'", playlist_name);
                        let target_path = get_playlist_path(&playlist_name, &default_playlist_path);
                        let mut target_playlist = Vec::new();
                        if target_path.exists() {
                            if let Ok(f) = File::open(&target_path) {
                                let reader = BufReader::new(f);
                                if let Ok(pl) = serde_json::from_reader::<_, Vec<TrackItem>>(reader) {
                                    target_playlist = pl;
                                }
                            }
                        }

                        target_playlist.retain(|t| {
                            if tracks.contains(&t.track_id) {
                                return false;
                            }
                            if let Some(ref aid) = t.album_id {
                                if albums.contains(aid) {
                                    return false;
                                }
                            }
                            for artist_id in &t.artist_ids {
                                if artists.contains(artist_id) {
                                    return false;
                                }
                            }
                            if playlist_tracks.contains(&t.track_id) {
                                return false;
                            }
                            true
                        });

                        save_playlist(&target_playlist, &Some(target_path.clone()));

                        let is_active = if let Some(ref curr_path) = current_playlist_path {
                            target_path == *curr_path
                        } else {
                            false
                        };
                        if is_active {
                            playlist = target_playlist;
                            next_preload = None;
                        }

                        let _ = resp.send(Ok(()));
                    }
                    Some(Command::DeletePlaylist { playlist_name, resp }) => {
                        println!("[CMD] DeletePlaylist '{}'", playlist_name);
                        let target_path = get_playlist_path(&playlist_name, &default_playlist_path);
                        if target_path.exists() {
                            if let Err(e) = std::fs::remove_file(&target_path) {
                                let _ = resp.send(Err(format!("Failed to delete playlist file: {}", e)));
                                return;
                            }
                        }

                        let is_active = if let Some(ref curr_path) = current_playlist_path {
                            target_path == *curr_path
                        } else {
                            false
                        };
                        if is_active {
                            current_playlist_path = default_playlist_path.clone();
                            playlist = Vec::new();
                            if let Some(gp) = &default_playlist_path {
                                if let Ok(f) = File::open(gp) {
                                    let reader = BufReader::new(f);
                                    if let Ok(pl) = serde_json::from_reader::<_, Vec<TrackItem>>(reader) {
                                        playlist = pl;
                                    }
                                }
                            }
                            playlist_idx = 0;
                            next_preload = None;
                        }

                        let _ = resp.send(Ok(()));
                    }
                    Some(Command::SkipCategory { category, resp }) => {
                        println!("[CMD] SkipCategory '{}'", category);
                        player.stop();

                        let mut final_item = None;
                        let mut queue_changed = false;

                        if let Some(ref cur) = current {
                            let cur_album_id = cur.album_id.clone();
                            let cur_artist_ids = cur.artist_ids.clone();

                            let mut search_count = 0;
                            while search_count < playlist.len() + 10 {
                                search_count += 1;
                                if let Some((pos, item)) = select_next(&playlist, playlist_idx) {
                                    let is_same = match category.as_str() {
                                        "album" => {
                                            item.album_id.is_some() && item.album_id == cur_album_id
                                        }
                                        "artist" => {
                                            item.artist_ids.iter().any(|id| cur_artist_ids.contains(id))
                                        }
                                        _ => false,
                                    };

                                    queue_changed |= consume_selected_track(
                                        &mut playlist,
                                        &mut playlist_idx,
                                        pos,
                                    );

                                    if !is_same {
                                        final_item = Some(item);
                                        break;
                                    }
                                } else {
                                    break;
                                }
                            }
                        } else if let Some((pos, item)) = select_next(&playlist, playlist_idx) {
                            queue_changed |= consume_selected_track(
                                &mut playlist,
                                &mut playlist_idx,
                                pos,
                            );
                            final_item = Some(item);
                        }

                        if queue_changed {
                            save_playlist(&playlist, &current_playlist_path);
                        }

                        if let Some(item) = final_item {
                            let sid = SpotifyId::from_base62(&item.track_id).unwrap();
                            let uri = SpotifyUri::Track { id: sid };
                            println!("[Playback] Skipping category directly to '{}'", item.track_name);
                            current = Some(item.clone());
                            state
                                .playback_timeline
                                .mark_next_buffer_after_restart_as_track_start(
                                    item.track_id.clone(),
                                );
                            player.load(uri, true, 0);
                            player.play();
                            next_preload = None;

                            let _ = resp.send(build_now_playing(
                                Some(&item),
                                "playing",
                                Some(0),
                                None,
                                state.active_listener_count(),
                                get_active_playlist_name(
                                    &current_playlist_path,
                                    &default_playlist_path,
                                ),
                            ));
                        } else {
                            current = None;
                            *state.start_time.lock().unwrap() = None;
                            *state.position_ms.lock().unwrap() = None;
                            let _ = resp.send(build_now_playing(
                                None,
                                "idle",
                                None,
                                None,
                                state.active_listener_count(),
                                get_active_playlist_name(
                                    &current_playlist_path,
                                    &default_playlist_path,
                                ),
                            ));
                        }
                    }
                    None => {
                        println!("[Playback] Command channel closed. Exiting.");
                        break;
                    }
                }
            }
            ev = ev_rx.recv() => {
                if let Some(event) = ev {
                    let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");

                    match event {
                        // **Capture duration when LibreSpot reports track metadata & Fetch Synced Lyrics**
                        PlayerEvent::TrackChanged { audio_item } => {
                            consecutive_unavailable_count = 0;
                            current_track_duration_ms = Some(audio_item.duration_ms);
                            let now = Instant::now();
                            position_ms_offset = 0;
                            start_instant = Some(now);
                            // Passthrough starts a new logical Ogg stream at granule zero for
                            // every track. GStreamer pipelines such as Opus keep one cumulative
                            // granule clock, so wait for their PCM marker instead of publishing
                            // a false zero-valued track origin during the transition.
                            track_start_samples = state.player_cfg.passthrough.then_some(0);
                            position_track_id = match &audio_item.track_id {
                                SpotifyUri::Track { id } => id.to_base62().ok(),
                                _ => current.as_ref().map(|track| track.track_id.clone()),
                            };
                            *state.start_time.lock().unwrap() = Some(now);
                            *state.position_ms.lock().unwrap() = Some(0);

                            if let Some(ref c) = current {
                                let np = build_now_playing(
                                    Some(c),
                                    "playing",
                                    Some(0),
                                    Some(audio_item.duration_ms),
                                    state.active_listener_count(),
                                    get_active_playlist_name(
                                        &current_playlist_path,
                                        &default_playlist_path,
                                    ),
                                );
                                let _ = state.tx_lyrics_ws.send(crate::models::WsMessage::NowPlaying(np));
                            }

                            // Spawn background task to fetch Spotify synced lyrics
                            let state_clone = state.clone();
                            let track_id_uri = audio_item.track_id.clone();
                            tokio::spawn(async move {
                                if let SpotifyUri::Track { id } = track_id_uri {
                                    println!("[Lyrics] Fetching synced lyrics for track {}", id.to_base62().unwrap());
                                    use librespot_metadata::lyrics::Lyrics;
                                     let session_cloned = state_clone.session.lock().unwrap().clone();
                                     match Lyrics::get(&session_cloned, &id).await {
                                        Ok(lyrics_reply) => {
                                            use librespot_metadata::lyrics::SyncType;
                                            if lyrics_reply.lyrics.sync_type == SyncType::LineSynced {
                                                let lines = lyrics_reply.lyrics.lines.into_iter().map(|line| {
                                                    let time_ms = line.start_time_ms.parse::<u32>().unwrap_or(0);
                                                    crate::models::SimpleLyricLine {
                                                        time_ms,
                                                        text: line.words,
                                                    }
                                                }).collect();

                                                let msg = crate::models::LyricsMessage::Lyrics {
                                                    track_id: id.to_base62().unwrap(),
                                                    background: lyrics_reply.colors.background as u32,
                                                    text_color: lyrics_reply.colors.text as u32,
                                                    highlight_color: lyrics_reply.colors.highlight_text as u32,
                                                    lines,
                                                };

                                                // Cache in State and broadcast to all connected WebSocket clients
                                                *state_clone.current_lyrics.lock().unwrap() = Some(msg.clone());
                                                let _ = state_clone.tx_lyrics_ws.send(msg);
                                                println!("[Lyrics] Synced lyrics successfully cached and broadcasted.");
                                            } else {
                                                println!("[Lyrics] Track has unsynced lyrics; not supported.");
                                                let msg = crate::models::LyricsMessage::NoLyrics;
                                                *state_clone.current_lyrics.lock().unwrap() = Some(msg.clone());
                                                let _ = state_clone.tx_lyrics_ws.send(msg);
                                            }
                                        }
                                        Err(e) => {
                                            println!("[Lyrics] Synced lyrics not available for this track: {}", e);
                                            let msg = crate::models::LyricsMessage::NoLyrics;
                                            *state_clone.current_lyrics.lock().unwrap() = Some(msg.clone());
                                            let _ = state_clone.tx_lyrics_ws.send(msg);
                                        }
                                    }
                                }
                            });
                        }
                        PlayerEvent::EndOfTrack { .. } | PlayerEvent::Unavailable { .. } => {
                            println!("[{timestamp}] [PlayerEvent] Event: {event:?}");

                            if matches!(event, PlayerEvent::Unavailable { .. }) {
                                consecutive_unavailable_count += 1;
                                println!("[Playback] PlayerEvent::Unavailable received (consecutive count: {}). Attempting to recreate session and player...", consecutive_unavailable_count);

                                if consecutive_unavailable_count >= 3 {
                                    println!("[Playback] 🚨 Detected {} consecutive Unavailable tracks. Adding 2-second settle delay for transient network settling before retrying...", consecutive_unavailable_count);
                                    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                                }

                                match recreate_session(&state).await {
                                    Ok(new_session) => {
                                        // Overwrite the session in state
                                        *state.session.lock().unwrap() = new_session.clone();

                                        // Recreate the player and event channel
                                        let state_for_sink = state.clone();
                                        player = Player::new(
                                            state.player_cfg.clone(),
                                            new_session,
                                            Box::new(NoOpVolume),
                                            move || new_streaming_sink(&state_for_sink),
                                        );
                                        ev_rx = player.get_player_event_channel();
                                        println!("[Playback] Session and Player successfully recreated after PlayerEvent::Unavailable.");
                                    }
                                    Err(e) => {
                                        eprintln!("[Playback] ⚠️ Failed to recreate session and player during recovery: {}", e);
                                    }
                                }
                            } else {
                                // Reset count on a clean EndOfTrack
                                consecutive_unavailable_count = 0;
                            }

                            if let Some((pos, item)) = select_next(&playlist, playlist_idx) {
                                let sid = SpotifyId::from_base62(&item.track_id).unwrap();
                                let uri = SpotifyUri::Track { id: sid };
                                current = Some(item.clone());
                                if playlist[pos].queue_idx.is_some() {
                                    playlist[pos].queue_idx = None;
                                    save_playlist(&playlist, &current_playlist_path);
                                } else {
                                    let lib_count = playlist.iter().filter(|i| i.queue_idx.is_none()).count();
                                    if lib_count > 0 {
                                        playlist_idx = (playlist_idx + 1) % lib_count;
                                    }
                                }
                                state
                                    .playback_timeline
                                    .mark_next_buffer_as_track_start(item.track_id.clone());
                                player.load(uri, true, 0);
                                player.play();
                                next_preload = None;
                            } else {
                                current = None;
                                *state.start_time.lock().unwrap() = None;
                                *state.position_ms.lock().unwrap() = None;
                                current_track_duration_ms = None;
                                position_ms_offset = 0;
                                start_instant = None;
                                track_start_samples = None;
                                position_track_id = None;
                                state.playback_timeline.clear();

                                // Broadcast Idle state
                                let msg = crate::models::LyricsMessage::Idle;
                                *state.current_lyrics.lock().unwrap() = Some(msg.clone());
                                let _ = state.tx_lyrics_ws.send(msg);

                                let np = build_now_playing(
                                    None,
                                    "idle",
                                    None,
                                    None,
                                    state.active_listener_count(),
                                    get_active_playlist_name(
                                        &current_playlist_path,
                                        &default_playlist_path,
                                    ),
                                );
                                let _ = state.tx_lyrics_ws.send(crate::models::WsMessage::NowPlaying(np));
                            }
                        }
                        PlayerEvent::PositionChanged { position_ms, track_id, .. } => {
                            let now = Instant::now();
                            *state.start_time.lock().unwrap() = Some(now);
                            *state.position_ms.lock().unwrap() = Some(position_ms);
                            position_ms_offset = position_ms;
                            start_instant = Some(now);
                            let current_total = *state.total_samples.lock().unwrap();
                            let offset_frames = (position_ms as u64 * 44100) / 1000;
                            track_start_samples = Some(current_total.saturating_sub(offset_frames));
                            position_track_id = match track_id {
                                SpotifyUri::Track { id } => id.to_base62().ok(),
                                _ => current.as_ref().map(|track| track.track_id.clone()),
                            };
                        }
                        _ => {
                            println!("[{timestamp}] [PlayerEvent] Event: {event:?}");
                        }
                    }
                }
            }
        }
    }
}

/// Creates a brand new, connected librespot session with the reusable playback credentials.
pub async fn recreate_session(
    state: &AppState,
) -> Result<librespot::core::session::Session, Box<dyn std::error::Error + Send + Sync>> {
    println!("[OAuth] Creating a fresh librespot session...");

    let cache_dir = crate::cache_paths::spotifm_cache_dir(state.cli.playlist_path.as_deref());

    let cache = librespot::core::cache::Cache::new(
        Some(cache_dir),
        None::<std::path::PathBuf>,
        None::<std::path::PathBuf>,
        None,
    )?;

    let credentials = cache.credentials().ok_or(
        "Reusable librespot playback credentials are missing; restart Spotifm to authorize playback",
    )?;
    let session = librespot::core::session::Session::new(
        librespot::core::config::SessionConfig::default(),
        Some(cache),
    );

    println!("[OAuth/Playback] Connecting a fresh session with reusable credentials...");
    librespot::core::session::Session::connect(&session, credentials, true).await?;
    println!("[OAuth] Fresh librespot session successfully connected.");

    Ok(session)
}
