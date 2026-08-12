use super::helpers::next_stream_session_id;
use crate::audio::stream_manager::OggPageIndex;
use crate::models::{StreamQuery, StreamSessionMeta};
use crate::state::AppState;
use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Arc;
use warp::Reply;

fn get_stream_content_type(cli: &crate::Cli) -> &'static str {
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

fn get_state_stream_content_type(state: &AppState) -> &'static str {
    if state.player_cfg.passthrough {
        "audio/ogg"
    } else {
        get_stream_content_type(&state.cli)
    }
}

struct OggConnectParser {
    enabled: bool,
    sid: String,
    parse_buffer: Vec<u8>,
    sessions: Arc<std::sync::Mutex<HashMap<String, StreamSessionMeta>>>,
    ogg_page_index: OggPageIndex,
    _guard: StreamSessionGuard,
}

struct StreamSessionGuard {
    sid: String,
    sessions: Arc<std::sync::Mutex<HashMap<String, StreamSessionMeta>>>,
    cmd_tx: tokio::sync::mpsc::UnboundedSender<crate::models::Command>,
}

impl StreamSessionGuard {
    fn new(
        sid: String,
        sessions: Arc<std::sync::Mutex<HashMap<String, StreamSessionMeta>>>,
        cmd_tx: tokio::sync::mpsc::UnboundedSender<crate::models::Command>,
    ) -> Self {
        {
            let mut sessions = sessions.lock().unwrap();
            let entry = sessions.entry(sid.clone()).or_default();
            entry.latest_granule_sec = None;
            entry.stream_origin_sec = None;
            entry.latest_granule_span_start_sec = None;
            entry.latest_granule_at = None;
            entry.last_seen = std::time::Instant::now();
            entry.active = true;
            entry.connection_count = entry.connection_count.saturating_add(1);
        }
        let _ = cmd_tx.send(crate::models::Command::BroadcastNowPlaying);

        Self {
            sid,
            sessions,
            cmd_tx,
        }
    }
}

impl Drop for StreamSessionGuard {
    fn drop(&mut self) {
        if let Ok(mut sessions) = self.sessions.lock() {
            if let Some(entry) = sessions.get_mut(&self.sid) {
                entry.connection_count = entry.connection_count.saturating_sub(1);
                entry.last_seen = std::time::Instant::now();
                entry.active = entry.connection_count > 0;
            }
        }
        let _ = self
            .cmd_tx
            .send(crate::models::Command::BroadcastNowPlaying);
    }
}

impl OggConnectParser {
    fn new(
        enabled: bool,
        sid: String,
        sessions: Arc<std::sync::Mutex<HashMap<String, StreamSessionMeta>>>,
        ogg_page_index: OggPageIndex,
        cmd_tx: tokio::sync::mpsc::UnboundedSender<crate::models::Command>,
    ) -> Self {
        let guard = StreamSessionGuard::new(sid.clone(), sessions.clone(), cmd_tx);
        Self {
            enabled,
            sid,
            parse_buffer: Vec::new(),
            sessions,
            ogg_page_index,
            _guard: guard,
        }
    }

    fn ingest(&mut self, bytes: &[u8]) {
        {
            let mut sessions = self.sessions.lock().unwrap();
            let entry = sessions.entry(self.sid.clone()).or_default();
            entry.last_seen = std::time::Instant::now();
            entry.active = true;
        }

        if !self.enabled {
            return;
        }

        self.parse_buffer.extend_from_slice(bytes);
        let granules =
            crate::audio::stream_manager::parse_ogg_page_granules(&mut self.parse_buffer);
        if !granules.is_empty() {
            let granule_rate = self.ogg_page_index.granule_rate();
            let mut sessions = self.sessions.lock().unwrap();
            let entry = sessions.entry(self.sid.clone()).or_default();
            entry.last_seen = std::time::Instant::now();
            entry.active = true;
            for granule in granules {
                if granule == 0 {
                    continue;
                }

                let granule_sec = granule as f64 / granule_rate;
                let span_start_sec = self
                    .ogg_page_index
                    .span_for_end_granule(granule)
                    .map(|span| span.start_sec)
                    .unwrap_or(granule_sec);
                if entry.stream_origin_sec.is_none() {
                    entry.stream_origin_sec = Some(span_start_sec);
                }
                entry.latest_granule_sec = Some(granule_sec);
                entry.latest_granule_span_start_sec = Some(span_start_sec);
                entry.latest_granule_at = Some(std::time::Instant::now());
            }
        }
    }
}

pub async fn handle_stream(
    query: StreamQuery,
    state: Arc<AppState>,
) -> Result<impl Reply, Infallible> {
    use futures_util::StreamExt;

    let session_id = query
        .sid
        .filter(|sid| !sid.trim().is_empty())
        .unwrap_or_else(next_stream_session_id);

    let rx = state.tx_mp3.subscribe();

    let live_stream = tokio_stream::wrappers::BroadcastStream::new(rx).map(|res| match res {
        Ok(bytes) => Ok::<_, std::convert::Infallible>(bytes),
        Err(_) => {
            // Underflow or lagging client: just push an empty slice to keep the connection alive
            Ok::<_, std::convert::Infallible>(bytes::Bytes::new())
        }
    });

    let content_type = get_state_stream_content_type(&state);
    let headers = state.stream_headers.lock().unwrap().clone();
    let header_stream =
        futures_util::stream::iter(headers.into_iter().map(Ok::<_, std::convert::Infallible>));
    let mut parser = OggConnectParser::new(
        content_type.contains("ogg"),
        session_id,
        state.stream_sessions.clone(),
        state.ogg_page_index.clone(),
        state.cmd_tx.clone(),
    );
    let live_stream = live_stream.map(move |res| {
        let bytes = res.expect("stream bytes should be infallible");
        parser.ingest(&bytes);
        Ok::<_, std::convert::Infallible>(bytes)
    });
    let chained_stream = header_stream.chain(live_stream);

    let body = warp::hyper::Body::wrap_stream(chained_stream);
    let mut response = warp::http::Response::builder()
        .header("content-type", content_type)
        .header("cache-control", "no-store")
        .header("access-control-allow-origin", "*")
        .header("access-control-allow-headers", "*")
        .header("access-control-allow-methods", "GET, OPTIONS");
    if content_type.contains("ogg") {
        response = response
            .header(
                "x-spotifm-ogg-granule-rate",
                state.ogg_page_index.granule_rate().to_string(),
            )
            .header(
                "access-control-expose-headers",
                "X-Spotifm-Ogg-Granule-Rate",
            );
    }
    let response = response.body(body).unwrap();

    Ok(response)
}
