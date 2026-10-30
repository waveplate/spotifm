use crate::models::{LyricsMessage, LyricsSource, SimpleLyricLine};
use librespot::core::{Session, SpotifyUri};
use librespot_metadata::audio::item::{AudioItem, UniqueFields};
use librespot_metadata::lyrics::{Lyrics, SyncType};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LrclibLyrics {
    synced_lyrics: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct CachedLrclibLyrics {
    version: u8,
    track_id: String,
    title: String,
    artist: String,
    album: String,
    duration_ms: u32,
    lines: Vec<SimpleLyricLine>,
}

async fn read_cache(path: PathBuf) -> Option<CachedLrclibLyrics> {
    tokio::task::spawn_blocking(move || {
        let file = std::fs::File::open(path).ok()?;
        serde_json::from_reader(file).ok()
    })
    .await
    .ok()
    .flatten()
}

async fn write_cache(path: PathBuf, entry: CachedLrclibLyrics) {
    // Complete the atomic write in a blocking task even if playback switches
    // tracks and cancels the network task while the cache is being saved.
    let result = tokio::task::spawn_blocking(move || -> std::io::Result<()> {
        let parent = path.parent().expect("Lyrics cache file has a parent");
        std::fs::create_dir_all(parent)?;
        let temporary = parent.join(format!(
            ".{}-{}-{:016x}.tmp",
            entry.track_id,
            std::process::id(),
            rand::random::<u64>()
        ));
        let data = serde_json::to_vec(&entry)?;
        let result =
            std::fs::write(&temporary, data).and_then(|_| std::fs::rename(&temporary, &path));
        if result.is_err() {
            let _ = std::fs::remove_file(temporary);
        }
        result
    })
    .await;
    match result {
        Ok(Ok(())) => {}
        Ok(Err(error)) => eprintln!("[Lyrics] Could not save LRCLIB cache: {error}"),
        Err(error) => eprintln!("[Lyrics] LRCLIB cache write failed: {error}"),
    }
}

fn lrclib_message(track_id: String, lines: Vec<SimpleLyricLine>) -> LyricsMessage {
    LyricsMessage::Lyrics {
        track_id,
        source: LyricsSource::Lrclib,
        background: 0xff121212,
        text_color: 0xffb3b3b3,
        highlight_color: 0xffffffff,
        lines,
    }
}

pub fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(concat!("spotifm/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(3))
        .timeout(Duration::from_secs(10))
        .build()
        .expect("Failed to build lyrics HTTP client")
}

pub async fn fetch(
    session: &Session,
    client: &reqwest::Client,
    audio_item: &AudioItem,
    cache_dir: &Path,
) -> LyricsMessage {
    let SpotifyUri::Track { id } = &audio_item.track_id else {
        return LyricsMessage::NoLyrics;
    };
    let Ok(track_id) = id.to_base62() else {
        return LyricsMessage::NoLyrics;
    };

    println!("[Lyrics] Fetching synced lyrics for track {track_id}");
    match tokio::time::timeout(Duration::from_secs(10), Lyrics::get(session, id)).await {
        Ok(Ok(reply)) if reply.lyrics.sync_type == SyncType::LineSynced => {
            let mut lines: Vec<_> = reply
                .lyrics
                .lines
                .into_iter()
                .filter_map(|line| {
                    Some(SimpleLyricLine {
                        time_ms: line.start_time_ms.parse().ok()?,
                        text: line.words,
                    })
                })
                .collect();
            if lines.iter().any(|line| !line.text.trim().is_empty()) {
                lines.sort_by_key(|line| line.time_ms);
                return LyricsMessage::Lyrics {
                    track_id,
                    source: LyricsSource::Spotify,
                    background: reply.colors.background as u32,
                    text_color: reply.colors.text as u32,
                    highlight_color: reply.colors.highlight_text as u32,
                    lines,
                };
            }
        }
        Ok(Err(error)) => println!("[Lyrics] Spotify lyrics unavailable: {error}"),
        Err(_) => println!("[Lyrics] Spotify lyrics request timed out."),
        _ => {}
    }

    let UniqueFields::Track { artists, album, .. } = &audio_item.unique_fields else {
        return LyricsMessage::NoLyrics;
    };
    let Some(artist) = artists.first() else {
        return LyricsMessage::NoLyrics;
    };
    let cache_path = cache_dir.join(format!("{track_id}.json"));
    if let Some(mut entry) = read_cache(cache_path.clone()).await {
        if entry.version == 1
            && entry.track_id == track_id
            && entry.title == audio_item.name
            && entry.artist == artist.name
            && entry.album == *album
            && entry.duration_ms == audio_item.duration_ms
            && entry.lines.iter().any(|line| !line.text.trim().is_empty())
        {
            entry.lines.sort_by_key(|line| line.time_ms);
            println!("[Lyrics] Using cached LRCLIB lyrics for track {track_id}");
            return lrclib_message(track_id, entry.lines);
        }
    }
    println!("[Lyrics] Trying LRCLIB for track {track_id}");
    match fetch_lrclib(
        client,
        "https://lrclib.net/api/get",
        &audio_item.name,
        &artist.name,
        album,
        audio_item.duration_ms,
    )
    .await
    {
        Ok(lines) if !lines.is_empty() => {
            println!("[Lyrics] Found synced lyrics on LRCLIB for track {track_id}");
            write_cache(
                cache_path,
                CachedLrclibLyrics {
                    version: 1,
                    track_id: track_id.clone(),
                    title: audio_item.name.clone(),
                    artist: artist.name.clone(),
                    album: album.clone(),
                    duration_ms: audio_item.duration_ms,
                    lines: lines.clone(),
                },
            )
            .await;
            lrclib_message(track_id, lines)
        }
        Ok(_) => LyricsMessage::NoLyrics,
        Err(error) => {
            eprintln!("[Lyrics] LRCLIB lookup failed: {error}");
            LyricsMessage::NoLyrics
        }
    }
}

async fn fetch_lrclib(
    client: &reqwest::Client,
    endpoint: &str,
    title: &str,
    artist: &str,
    album: &str,
    duration_ms: u32,
) -> Result<Vec<SimpleLyricLine>, reqwest::Error> {
    let duration = (f64::from(duration_ms) / 1000.0).to_string();
    let response = client
        .get(endpoint)
        .query(&[
            ("track_name", title),
            ("artist_name", artist),
            ("album_name", album),
            ("duration", duration.as_str()),
        ])
        .send()
        .await?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(Vec::new());
    }
    let lyrics: LrclibLyrics = response.error_for_status()?.json().await?;
    Ok(lyrics
        .synced_lyrics
        .as_deref()
        .map(parse_lrc)
        .unwrap_or_default())
}

fn parse_timestamp(timestamp: &str) -> Option<u32> {
    let (minutes, seconds) = timestamp.split_once(':')?;
    let (seconds, fraction) = seconds.split_once('.').unwrap_or((seconds, ""));
    if minutes.is_empty()
        || seconds.is_empty()
        || fraction.len() > 3
        || !minutes
            .bytes()
            .chain(seconds.bytes())
            .chain(fraction.bytes())
            .all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    let seconds = seconds.parse::<u32>().ok()?;
    if seconds >= 60 {
        return None;
    }
    let milliseconds = if fraction.is_empty() {
        0
    } else {
        fraction.parse::<u32>().ok()? * 10u32.pow(3 - fraction.len() as u32)
    };
    minutes
        .parse::<u32>()
        .ok()?
        .checked_mul(60_000)?
        .checked_add(seconds * 1000)?
        .checked_add(milliseconds)
}

fn parse_lrc(lrc: &str) -> Vec<SimpleLyricLine> {
    // Positive LRC offsets display lyrics earlier. The offset applies to every
    // timestamp, regardless of where the metadata appears in the file.
    let offset = lrc
        .lines()
        .filter_map(|line| {
            line.trim()
                .strip_prefix("[offset:")?
                .strip_suffix(']')?
                .trim()
                .parse::<i64>()
                .ok()
        })
        .last()
        .unwrap_or(0);
    let mut lines = Vec::new();
    for line in lrc.lines() {
        let mut remainder = line.trim().trim_start_matches('\u{feff}');
        let mut timestamps = Vec::new();
        while let Some(tag) = remainder.strip_prefix('[') {
            let Some((timestamp, rest)) = tag.split_once(']') else {
                break;
            };
            let Some(time_ms) = parse_timestamp(timestamp) else {
                break;
            };
            timestamps.push(time_ms);
            remainder = rest;
        }
        for time_ms in timestamps {
            lines.push(SimpleLyricLine {
                time_ms: i64::from(time_ms)
                    .saturating_sub(offset)
                    .clamp(0, i64::from(u32::MAX)) as u32,
                text: remainder.trim().to_string(),
            });
        }
    }
    // Keep empty timed lines to clear the display during instrumental breaks.
    if !lines.iter().any(|line| !line.text.is_empty()) {
        return Vec::new();
    }
    lines.sort_by_key(|line| line.time_ms);
    lines
}

#[cfg(test)]
mod tests {
    use super::{fetch_lrclib, http_client, parse_lrc};
    use warp::Filter;

    #[test]
    fn parses_fractional_and_repeated_timestamps_in_order_with_gaps() {
        let lines =
            parse_lrc("[ar:Artist]\n[00:12.345][01:02.34] Chorus\n[00:03.4]First\n[00:05]\n");
        let lines: Vec<_> = lines
            .iter()
            .map(|line| (line.time_ms, line.text.as_str()))
            .collect();
        assert_eq!(
            lines,
            vec![
                (3400, "First"),
                (5000, ""),
                (12345, "Chorus"),
                (62340, "Chorus")
            ]
        );
    }

    #[test]
    fn ignores_invalid_timestamps_and_unsynced_lyrics() {
        assert!(parse_lrc("[ti:Title]\nUntimed lyrics\n[00:60.00]Invalid\n[NaN:01]Invalid\n[999999999:00]Overflow\n[00:01.1234]Invalid").is_empty());
        assert!(parse_lrc("[00:01.00]\n[00:02.00] ").is_empty());
    }

    #[test]
    fn applies_global_offset_and_clamps_negative_times() {
        let lines = parse_lrc("[00:00.1]First\n[00:02.5]Second\n[offset:+500]");
        assert_eq!(lines[0].time_ms, 0);
        assert_eq!(lines[1].time_ms, 2000);
        assert_eq!(parse_lrc("[offset:-500]\n[00:01.0]Later")[0].time_ms, 1500);
    }

    #[tokio::test]
    async fn lrclib_query_encodes_metadata_and_duration_in_seconds() {
        let route = warp::path!("api" / "get")
            .and(warp::query::<std::collections::HashMap<String, String>>())
            .and(warp::header::<String>("user-agent"))
            .map(
                |query: std::collections::HashMap<String, String>, agent: String| {
                    assert_eq!(query["track_name"], "Title & more?");
                    assert_eq!(query["artist_name"], "Björk");
                    assert_eq!(query["album_name"], "Album / Deluxe");
                    assert_eq!(query["duration"], "123.456");
                    assert!(agent.starts_with("spotifm/"));
                    warp::reply::json(&serde_json::json!({"syncedLyrics":"[00:01.23]Example"}))
                },
            );
        let (address, server) = warp::serve(route).bind_ephemeral(([127, 0, 0, 1], 0));
        tokio::spawn(server);
        let lines = fetch_lrclib(
            &http_client(),
            &format!("http://{address}/api/get"),
            "Title & more?",
            "Björk",
            "Album / Deluxe",
            123456,
        )
        .await
        .unwrap();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].time_ms, 1230);
        assert_eq!(lines[0].text, "Example");
    }

    #[tokio::test]
    async fn lrclib_missing_and_plain_lyrics_return_no_timed_lines() {
        let route = warp::path::param::<String>().map(|kind: String| {
            let (body, status) = match kind.as_str() {
                "missing" => (
                    serde_json::json!({"name":"TrackNotFound"}),
                    warp::http::StatusCode::NOT_FOUND,
                ),
                _ => (
                    serde_json::json!({"syncedLyrics":null,"plainLyrics":"Untimed lyrics"}),
                    warp::http::StatusCode::OK,
                ),
            };
            warp::reply::with_status(warp::reply::json(&body), status)
        });
        let (address, server) = warp::serve(route).bind_ephemeral(([127, 0, 0, 1], 0));
        tokio::spawn(server);
        for kind in ["missing", "plain"] {
            assert!(fetch_lrclib(
                &http_client(),
                &format!("http://{address}/{kind}"),
                "Title",
                "Artist",
                "Album",
                123000
            )
            .await
            .unwrap()
            .is_empty());
        }
    }
}
