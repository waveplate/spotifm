use crate::models::{LyricsMessage, WsMessage, WsQuery};
use crate::state::AppState;
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Instant;
use warp::Reply;

fn estimated_session_granule_sec(meta: &crate::models::StreamSessionMeta) -> Option<f64> {
    let end_sec = meta.latest_granule_sec?;
    let Some(start_sec) = meta.latest_granule_span_start_sec else {
        return Some(end_sec);
    };
    let Some(observed_at) = meta.latest_granule_at else {
        return Some(end_sec);
    };

    let span_duration_sec = (end_sec - start_sec).max(0.0);
    let elapsed_sec = Instant::now()
        .saturating_duration_since(observed_at)
        .as_secs_f64()
        .min(span_duration_sec);
    Some(start_sec + elapsed_sec)
}

pub async fn handle_lyrics_ws(
    ws: warp::ws::Ws,
    query: WsQuery,
    state: Arc<AppState>,
) -> Result<impl Reply, Infallible> {
    let sid = query.sid.filter(|sid| !sid.trim().is_empty());
    Ok(ws.on_upgrade(move |socket| handle_lyrics_connection(socket, sid, state)))
}

pub async fn handle_lyrics_rest(state: Arc<AppState>) -> Result<impl Reply, Infallible> {
    let current = state.current_lyrics.lock().unwrap().clone();

    let response = match current {
        Some(msg) => warp::http::Response::builder()
            .header("content-type", "application/json; charset=utf-8")
            .header("access-control-allow-origin", "*")
            .header("access-control-allow-headers", "*")
            .header("access-control-allow-methods", "GET, OPTIONS")
            .body(serde_json::to_string(&msg).unwrap())
            .unwrap(),
        None => warp::http::Response::builder()
            .header("content-type", "application/json; charset=utf-8")
            .header("access-control-allow-origin", "*")
            .header("access-control-allow-headers", "*")
            .header("access-control-allow-methods", "GET, OPTIONS")
            .body(serde_json::to_string(&LyricsMessage::Idle).unwrap())
            .unwrap(),
    };

    Ok(response)
}

fn sync_position_for_session(
    msg: LyricsMessage,
    sid: Option<&str>,
    state: &AppState,
) -> LyricsMessage {
    let Some(sid) = sid else {
        return msg;
    };

    match msg {
        LyricsMessage::Position {
            position_ms,
            track_id,
            start_granule_sec,
            stream_origin_sec,
        } => {
            let session_meta = {
                let sessions = state.stream_sessions.lock().unwrap();
                sessions.get(sid).cloned()
            };

            let output_position_ms = start_granule_sec.and_then(|track_start_sec| {
                let latest_granule_sec = estimated_session_granule_sec(session_meta.as_ref()?)?;
                let position_sec = latest_granule_sec - track_start_sec;
                if position_sec.is_finite() {
                    Some((position_sec.max(0.0) * 1000.0) as u32)
                } else {
                    None
                }
            });
            let output_stream_origin_sec = session_meta
                .and_then(|meta| meta.stream_origin_sec)
                .or(stream_origin_sec);

            LyricsMessage::Position {
                position_ms: output_position_ms.unwrap_or(position_ms),
                track_id,
                start_granule_sec,
                stream_origin_sec: output_stream_origin_sec,
            }
        }
        other => other,
    }
}

async fn handle_lyrics_connection(
    socket: warp::ws::WebSocket,
    sid: Option<String>,
    state: Arc<AppState>,
) {
    use futures_util::{SinkExt, StreamExt};
    use tokio::sync::broadcast::error::RecvError;
    use tokio::sync::Mutex;

    println!("[WS] 🔌 New lyrics WebSocket client connected.");
    let (ws_tx, mut ws_rx) = socket.split();
    let ws_tx = Arc::new(Mutex::new(ws_tx));

    // 1. Instantly send current NowPlaying state on connection so the
    // client can validate any cached lyrics against the active track.
    let (np_tx, np_rx) = tokio::sync::oneshot::channel();
    let mut current_is_idle = true;
    if state
        .cmd_tx
        .send(crate::models::Command::GetNowPlaying { resp: np_tx })
        .is_ok()
    {
        if let Ok(np) = np_rx.await {
            current_is_idle = np.status == "idle";
            if let Ok(json) = serde_json::to_string(&WsMessage::NowPlaying(np)) {
                let _ = ws_tx.lock().await.send(warp::ws::Message::text(json)).await;
            }
        }
    }

    // 1.5. Send the cached now-playing lyrics after the track identity.
    let cached_lyrics = state.current_lyrics.lock().unwrap().clone();
    if let Some(msg) = cached_lyrics {
        if let Ok(json) = serde_json::to_string(&msg) {
            let _ = ws_tx.lock().await.send(warp::ws::Message::text(json)).await;
        }
    } else {
        let placeholder = if current_is_idle {
            LyricsMessage::Idle
        } else {
            LyricsMessage::NoLyrics
        };
        if let Ok(json) = serde_json::to_string(&placeholder) {
            let _ = ws_tx.lock().await.send(warp::ws::Message::text(json)).await;
        }
    }

    // 2. Subscribe to the state's lyrics broadcast channel
    let mut rx_lyrics = state.tx_lyrics_ws.subscribe();

    // 3. Spawn a background task to process incoming client messages (queries)
    let ws_tx_clone = ws_tx.clone();
    let state_clone = state.clone();
    tokio::spawn(async move {
        use crate::models::Command;
        use tokio::sync::oneshot;

        #[derive(serde::Deserialize)]
        struct ClientQuery {
            action: String,
        }

        while let Some(Ok(msg)) = ws_rx.next().await {
            if msg.is_close() {
                break;
            }
            if let Ok(text) = msg.to_str() {
                if let Ok(query) = serde_json::from_str::<ClientQuery>(text) {
                    match query.action.as_str() {
                        "get_now_playing" => {
                            let (tx, rx) = oneshot::channel();
                            if state_clone
                                .cmd_tx
                                .send(Command::GetNowPlaying { resp: tx })
                                .is_ok()
                            {
                                if let Ok(np) = rx.await {
                                    let resp_msg = WsMessage::NowPlaying(np);
                                    if let Ok(json) = serde_json::to_string(&resp_msg) {
                                        let _ = ws_tx_clone
                                            .lock()
                                            .await
                                            .send(warp::ws::Message::text(json))
                                            .await;
                                    }
                                }
                            }
                        }
                        "get_next" => {
                            let (tx, rx) = oneshot::channel();
                            if state_clone
                                .cmd_tx
                                .send(Command::GetNext { resp: tx })
                                .is_ok()
                            {
                                if let Ok(next) = rx.await {
                                    let resp_msg = WsMessage::Next { next };
                                    if let Ok(json) = serde_json::to_string(&resp_msg) {
                                        let _ = ws_tx_clone
                                            .lock()
                                            .await
                                            .send(warp::ws::Message::text(json))
                                            .await;
                                    }
                                }
                            }
                        }
                        "get_playlist" => {
                            let (tx, rx) = oneshot::channel();
                            if state_clone
                                .cmd_tx
                                .send(Command::GetActivePlaylist { resp: tx })
                                .is_ok()
                            {
                                if let Ok(playlist) = rx.await {
                                    let resp_msg = WsMessage::Playlist(playlist);
                                    if let Ok(json) = serde_json::to_string(&resp_msg) {
                                        let _ = ws_tx_clone
                                            .lock()
                                            .await
                                            .send(warp::ws::Message::text(json))
                                            .await;
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        println!("[WS] 🔌 Client connection closed or dropped.");
    });

    // 4. Main loop: Receive broadcast messages and forward them to the client WebSocket
    loop {
        match rx_lyrics.recv().await {
            Ok(msg) => {
                let msg = sync_position_for_session(msg, sid.as_deref(), &state);
                if let Ok(json) = serde_json::to_string(&msg) {
                    if ws_tx
                        .lock()
                        .await
                        .send(warp::ws::Message::text(json))
                        .await
                        .is_err()
                    {
                        // Connection was closed by client
                        break;
                    }
                }
            }
            Err(RecvError::Lagged(skipped)) => {
                println!(
                    "[WS] Lyrics WebSocket lagged by {} message(s). Sending a fresh sync snapshot.",
                    skipped
                );

                let cached_lyrics = { state.current_lyrics.lock().unwrap().clone() };
                if let Some(msg) = cached_lyrics {
                    if let Ok(json) = serde_json::to_string(&msg) {
                        if ws_tx
                            .lock()
                            .await
                            .send(warp::ws::Message::text(json))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                }
            }
            Err(RecvError::Closed) => break,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::estimated_session_granule_sec;
    use crate::models::StreamSessionMeta;
    use std::time::{Duration, Instant};

    #[test]
    fn estimates_progress_within_the_latest_ogg_page() {
        let meta = StreamSessionMeta {
            latest_granule_sec: Some(10.5),
            latest_granule_span_start_sec: Some(10.0),
            latest_granule_at: Some(Instant::now() - Duration::from_millis(250)),
            ..StreamSessionMeta::default()
        };

        let estimated = estimated_session_granule_sec(&meta).unwrap();
        assert!((10.24..=10.30).contains(&estimated));
    }

    #[test]
    fn does_not_estimate_past_the_latest_ogg_page() {
        let meta = StreamSessionMeta {
            latest_granule_sec: Some(10.5),
            latest_granule_span_start_sec: Some(10.0),
            latest_granule_at: Some(Instant::now() - Duration::from_secs(2)),
            ..StreamSessionMeta::default()
        };

        assert_eq!(estimated_session_granule_sec(&meta), Some(10.5));
    }
}
