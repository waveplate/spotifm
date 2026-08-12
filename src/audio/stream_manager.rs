use bytes::Bytes;
use gst::prelude::*;
use gstreamer as gst;
use gstreamer_app as gst_app;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::broadcast;
use tokio::sync::Notify;

use librespot::playback::audio_backend::{Sink, SinkError, SinkResult};
use librespot::playback::convert::Converter;
use librespot::playback::decoder::AudioPacket;

const SAMPLE_RATE: usize = 44100;
const VORBIS_GRANULE_RATE: f64 = 44_100.0;
const CHANNELS: usize = 2;
const MAX_PIPELINE_LAG_SECONDS: usize = 10;
const MAX_QUEUED_PCM_FRAMES: usize = SAMPLE_RATE * MAX_PIPELINE_LAG_SECONDS;
const MAX_OGG_PAGE_SPANS: usize = 4096;

struct PcmBuffer {
    samples: Vec<i16>,
    track_start_id: Option<String>,
}

struct PcmQueue {
    buffers: VecDeque<PcmBuffer>,
    queued_frames: usize,
    closed: bool,
}

struct PcmQueueShared {
    queue: Mutex<PcmQueue>,
    notify: Notify,
    last_drop_log: Mutex<Option<Instant>>,
}

#[derive(Clone)]
pub struct PcmSender {
    shared: Arc<PcmQueueShared>,
}

struct PcmReceiver {
    shared: Arc<PcmQueueShared>,
}

fn pcm_frames(samples: &[i16]) -> usize {
    samples.len() / CHANNELS
}

#[derive(Clone, Debug)]
pub struct TrackStart {
    pub track_id: String,
    pub stream_sec: f64,
}

#[derive(Default)]
struct PlaybackTimelineInner {
    pending_track_start: Option<String>,
    pending_track_start_after_restart: Option<String>,
    current_track_start: Option<TrackStart>,
}

#[derive(Clone, Default)]
pub struct PlaybackTimeline {
    inner: Arc<Mutex<PlaybackTimelineInner>>,
}

impl PlaybackTimeline {
    pub fn mark_next_buffer_as_track_start(&self, track_id: String) {
        let mut inner = self.inner.lock().unwrap();
        inner.pending_track_start = Some(track_id);
    }

    /// Mark a track boundary that follows an explicit player stop.
    ///
    /// `Player::stop()` is asynchronous, so an old-track PCM packet can still reach the sink
    /// after the command is sent. Keep this marker disarmed until librespot starts the sink again
    /// to prevent that residual packet from being labelled as the new track's first packet.
    pub fn mark_next_buffer_after_restart_as_track_start(&self, track_id: String) {
        let mut inner = self.inner.lock().unwrap();
        inner.pending_track_start = None;
        inner.pending_track_start_after_restart = Some(track_id);
    }

    fn arm_track_start_after_restart(&self) -> bool {
        let mut inner = self.inner.lock().unwrap();
        if let Some(track_id) = inner.pending_track_start_after_restart.take() {
            inner.pending_track_start = Some(track_id);
            true
        } else {
            false
        }
    }

    fn take_pending_track_start(&self) -> Option<String> {
        let mut inner = self.inner.lock().unwrap();
        inner.pending_track_start.take()
    }

    fn record_track_start(&self, track_id: String, stream_sec: f64) {
        let mut inner = self.inner.lock().unwrap();
        inner.current_track_start = Some(TrackStart {
            track_id,
            stream_sec,
        });
    }

    pub fn current_track_start(&self) -> Option<TrackStart> {
        self.inner.lock().unwrap().current_track_start.clone()
    }

    pub fn clear(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.pending_track_start = None;
        inner.pending_track_start_after_restart = None;
        inner.current_track_start = None;
    }
}

#[derive(Clone, Copy)]
pub struct OggPageSpan {
    pub end_granule: u64,
    pub start_sec: f64,
}

struct OggPageIndexInner {
    parse_buffer: Vec<u8>,
    last_granule: Option<u64>,
    spans: VecDeque<OggPageSpan>,
}

#[derive(Clone)]
pub struct OggPageIndex {
    granule_rate: f64,
    inner: Arc<Mutex<OggPageIndexInner>>,
}

impl OggPageIndex {
    fn new(granule_rate: f64) -> Self {
        Self {
            granule_rate,
            inner: Arc::new(Mutex::new(OggPageIndexInner {
                parse_buffer: Vec::new(),
                last_granule: None,
                spans: VecDeque::new(),
            })),
        }
    }

    pub fn granule_rate(&self) -> f64 {
        self.granule_rate
    }

    pub fn ingest(&self, bytes: &[u8]) {
        let mut inner = self.inner.lock().unwrap();
        inner.parse_buffer.extend_from_slice(bytes);
        let pages = parse_ogg_page_granules(&mut inner.parse_buffer);

        for granule in pages {
            if granule == 0 {
                inner.last_granule = Some(0);
                continue;
            }

            let start_granule = inner.last_granule.unwrap_or(granule);
            let span = OggPageSpan {
                end_granule: granule,
                start_sec: start_granule as f64 / self.granule_rate,
            };

            inner.spans.push_back(span);
            while inner.spans.len() > MAX_OGG_PAGE_SPANS {
                inner.spans.pop_front();
            }

            inner.last_granule = Some(granule);
        }
    }

    pub fn span_for_end_granule(&self, end_granule: u64) -> Option<OggPageSpan> {
        let inner = self.inner.lock().unwrap();
        inner
            .spans
            .iter()
            .rev()
            .find(|span| span.end_granule == end_granule)
            .copied()
    }
}

pub fn ogg_granule_rate_for_pipeline(pipeline_name: &str) -> f64 {
    if pipeline_name.to_lowercase().contains("vorbis") {
        VORBIS_GRANULE_RATE
    } else {
        48_000.0
    }
}

#[derive(Clone)]
struct ParsedOggPage {
    granule_pos: Option<u64>,
    header_type: u8,
    serial: u32,
    bytes: Bytes,
}

fn parse_ogg_pages(buf: &mut Vec<u8>) -> Vec<ParsedOggPage> {
    let mut offset = 0usize;
    let mut pages = Vec::new();

    while offset + 27 <= buf.len() {
        if &buf[offset..offset + 4] != b"OggS" {
            offset += 1;
            continue;
        }

        let nsegs = buf[offset + 26] as usize;
        if offset + 27 + nsegs > buf.len() {
            break;
        }

        let body_size: usize = buf[offset + 27..offset + 27 + nsegs]
            .iter()
            .map(|x| *x as usize)
            .sum();
        let total_page_size = 27 + nsegs + body_size;

        if offset + total_page_size > buf.len() {
            break;
        }

        let raw_granule = u64::from_le_bytes(
            buf[offset + 6..offset + 14]
                .try_into()
                .expect("Ogg page granule slice should be 8 bytes"),
        );
        let serial = u32::from_le_bytes(
            buf[offset + 14..offset + 18]
                .try_into()
                .expect("Ogg page serial slice should be 4 bytes"),
        );
        let page = ParsedOggPage {
            granule_pos: (raw_granule != u64::MAX).then_some(raw_granule),
            header_type: buf[offset + 5],
            serial,
            bytes: Bytes::copy_from_slice(&buf[offset..offset + total_page_size]),
        };
        pages.push(page);

        offset += total_page_size;
    }

    if offset > 0 {
        let remaining = buf[offset..].to_vec();
        *buf = remaining;
    }

    pages
}

pub fn parse_ogg_page_granules(buf: &mut Vec<u8>) -> Vec<u64> {
    parse_ogg_pages(buf)
        .into_iter()
        .filter_map(|page| page.granule_pos)
        .collect()
}

fn bounded_pcm_channel() -> (PcmSender, PcmReceiver) {
    let shared = Arc::new(PcmQueueShared {
        queue: Mutex::new(PcmQueue {
            buffers: VecDeque::new(),
            queued_frames: 0,
            closed: false,
        }),
        notify: Notify::new(),
        last_drop_log: Mutex::new(None),
    });

    (
        PcmSender {
            shared: shared.clone(),
        },
        PcmReceiver { shared },
    )
}

impl PcmSender {
    fn send_with_marker(
        &self,
        mut samples: Vec<i16>,
        mut track_start_id: Option<String>,
    ) -> Result<(), ()> {
        let mut dropped_frames = 0usize;
        let mut frames = pcm_frames(&samples);

        if frames > MAX_QUEUED_PCM_FRAMES {
            let keep_samples = MAX_QUEUED_PCM_FRAMES * CHANNELS;
            let start = samples.len().saturating_sub(keep_samples);
            dropped_frames += frames - MAX_QUEUED_PCM_FRAMES;
            samples = samples[start..].to_vec();
            frames = pcm_frames(&samples);
            track_start_id = None;
        }

        {
            let mut queue = self.shared.queue.lock().unwrap();
            if queue.closed {
                return Err(());
            }

            while queue.queued_frames + frames > MAX_QUEUED_PCM_FRAMES {
                if let Some(old) = queue.buffers.pop_front() {
                    let old_frames = pcm_frames(&old.samples);
                    queue.queued_frames = queue.queued_frames.saturating_sub(old_frames);
                    dropped_frames += old_frames;
                } else {
                    break;
                }
            }

            queue.queued_frames += frames;
            queue.buffers.push_back(PcmBuffer {
                samples,
                track_start_id,
            });
        }

        if dropped_frames > 0 {
            let mut last = self.shared.last_drop_log.lock().unwrap();
            let now = Instant::now();
            if last.is_none_or(|previous| now.duration_since(previous) >= Duration::from_secs(5)) {
                println!(
                    "[StreamManager] Dropped {:.2}s of queued PCM to keep server pipeline lag under {}s.",
                    dropped_frames as f64 / SAMPLE_RATE as f64,
                    MAX_PIPELINE_LAG_SECONDS
                );
                *last = Some(now);
            }
        }

        self.shared.notify.notify_one();
        Ok(())
    }

    fn discard_queued(&self) -> usize {
        let mut queue = self.shared.queue.lock().unwrap();
        let discarded_frames = queue.queued_frames;
        queue.buffers.clear();
        queue.queued_frames = 0;
        discarded_frames
    }
}

impl PcmReceiver {
    async fn recv(&mut self) -> Option<PcmBuffer> {
        loop {
            let notified = {
                let mut queue = self.shared.queue.lock().unwrap();
                if let Some(buffer) = queue.buffers.pop_front() {
                    queue.queued_frames = queue
                        .queued_frames
                        .saturating_sub(pcm_frames(&buffer.samples));
                    return Some(buffer);
                }
                if queue.closed {
                    return None;
                }
                self.shared.notify.notified()
            };

            notified.await;
        }
    }
}

impl Drop for PcmReceiver {
    fn drop(&mut self) {
        let mut queue = self.shared.queue.lock().unwrap();
        queue.closed = true;
        self.shared.notify.notify_waiters();
    }
}

/// A thread-safe manager for our GStreamer pipeline and subscribers.
#[derive(Clone)]
pub struct StreamManager {
    pub tx_pcm: PcmSender,
    pub tx_mp3: broadcast::Sender<Bytes>,
    pub stream_headers: Arc<Mutex<Vec<Bytes>>>,
    pub playback_timeline: PlaybackTimeline,
    pub ogg_page_index: OggPageIndex,
}

pub struct StreamManagerConfig {
    pub pipeline_name: String,
    pub bitrate: u32,
    pub queue_size: u32,
    pub max_buffers: u32,
    pub silence_interval_ms: u64,
    pub custom_pipeline: String,
    pub passthrough: bool,
}

impl StreamManager {
    /// Initialize GStreamer and build the encoding/broadcasting pipeline with dynamic performance settings.
    pub fn new(
        config: StreamManagerConfig,
        total_samples: Arc<Mutex<u64>>,
    ) -> Result<Self, String> {
        let StreamManagerConfig {
            pipeline_name,
            bitrate,
            queue_size,
            max_buffers,
            silence_interval_ms,
            custom_pipeline,
            passthrough,
        } = config;

        if passthrough {
            let (tx_pcm, _rx_pcm) = bounded_pcm_channel();
            let (tx_mp3, _) = broadcast::channel::<Bytes>(queue_size as usize);
            println!(
                "[StreamManager] Librespot Ogg/Vorbis passthrough enabled. GStreamer stream encoder is disabled."
            );

            return Ok(Self {
                tx_pcm,
                tx_mp3,
                stream_headers: Arc::new(Mutex::new(Vec::new())),
                playback_timeline: PlaybackTimeline::default(),
                ogg_page_index: OggPageIndex::new(VORBIS_GRANULE_RATE),
            });
        }

        // Initialize GStreamer
        gst::init().map_err(|e| format!("Failed to initialize GStreamer: {:?}", e))?;

        // Construct the pipeline.
        let pipeline_str = custom_pipeline;
        let pipeline = gst::parse::launch(&pipeline_str)
            .map_err(|e| format!("Failed to parse GStreamer pipeline: {:?}", e))?;

        let pipeline = pipeline
            .dynamic_cast::<gst::Pipeline>()
            .map_err(|_| "Failed to cast parsed GStreamer element to Pipeline".to_string())?;

        let appsrc = pipeline
            .by_name("src")
            .ok_or_else(|| "src element not found in GStreamer pipeline".to_string())?
            .dynamic_cast::<gst_app::AppSrc>()
            .map_err(|_| "src element is not an AppSrc".to_string())?;

        let appsink = pipeline
            .by_name("sink")
            .ok_or_else(|| "sink element not found in GStreamer pipeline".to_string())?
            .dynamic_cast::<gst_app::AppSink>()
            .map_err(|_| "sink element is not an AppSink".to_string())?;

        // Start the GStreamer pipeline
        pipeline
            .set_state(gst::State::Playing)
            .map_err(|e| format!("Failed to set GStreamer pipeline to PLAYING: {:?}", e))?;

        println!(
            "[StreamManager] GStreamer pipeline '{}' initialized and PLAYING (bitrate={} kbps, max_buffers={}).",
            pipeline_name, bitrate, max_buffers
        );

        // Channels for communication
        let (tx_pcm, mut rx_pcm) = bounded_pcm_channel();
        let (tx_mp3, _) = broadcast::channel::<Bytes>(queue_size as usize);
        let playback_timeline = PlaybackTimeline::default();
        let ogg_page_index = OggPageIndex::new(ogg_granule_rate_for_pipeline(&pipeline_str));

        let tx_mp3_clone = tx_mp3.clone();
        let stream_headers = Arc::new(Mutex::new(Vec::new()));
        let stream_headers_clone = stream_headers.clone();

        // Task A: Loop reading PCM data from Spotify's Player and pushing to GStreamer appsrc.
        // Also automatically generates silence if no PCM is received within silence_interval_ms to keep the stream continuous.
        let total_samples_clone = total_samples.clone();
        let playback_timeline_clone = playback_timeline.clone();
        tokio::spawn(async move {
            let bytes_per_sample = 2;
            let bytes_per_sec = SAMPLE_RATE * CHANNELS * bytes_per_sample; // 176,400 bytes/sec

            let chunk_duration_ms = silence_interval_ms;
            let chunk_size = (bytes_per_sec * chunk_duration_ms as usize) / 1000;
            let silence_chunk = vec![0u8; chunk_size];

            let mut last_push = std::time::Instant::now();

            loop {
                tokio::select! {
                    samples = rx_pcm.recv() => {
                        let Some(buffer) = samples else {
                            println!("[StreamManager] PCM channel disconnected. Exiting feed loop.");
                            break;
                        };
                        let PcmBuffer {
                            samples,
                            track_start_id,
                        } = buffer;
                        let num_frames = samples.len() / CHANNELS;
                        {
                            let mut total = total_samples_clone.lock().unwrap();
                            if let Some(track_id) = track_start_id {
                                playback_timeline_clone
                                    .record_track_start(track_id, *total as f64 / SAMPLE_RATE as f64);
                            }
                            *total += num_frames as u64;
                        }
                        let mut bytes = Vec::with_capacity(samples.len() * 2);
                        for s in samples {
                            bytes.extend_from_slice(&s.to_le_bytes());
                        }
                        // Keep appsrc buffers untimestamped and let the audio encoder derive its
                        // native clock from the sample stream. Browsers differ on whether a live
                        // Ogg audio.currentTime exposes that cumulative granule clock or a timeline
                        // relative to the HTTP connection, so the player handles both forms.
                        let buffer = gst::Buffer::from_slice(bytes);
                        if let Err(error) = appsrc.push_buffer(buffer) {
                            eprintln!("[StreamManager] Failed to push PCM into GStreamer: {error}");
                        }
                        last_push = std::time::Instant::now();
                    }
                    _ = tokio::time::sleep(tokio::time::Duration::from_millis(5)) => {
                        let now = std::time::Instant::now();
                        if now.duration_since(last_push).as_millis() >= chunk_duration_ms as u128 {
                            let num_frames = silence_chunk.len()
                                / (CHANNELS * bytes_per_sample);
                            let buffer = gst::Buffer::from_slice(silence_chunk.clone());
                            if let Err(error) = appsrc.push_buffer(buffer) {
                                eprintln!("[StreamManager] Failed to push silence into GStreamer: {error}");
                            }
                            {
                                let mut total = total_samples_clone.lock().unwrap();
                                *total += num_frames as u64;
                            }
                            last_push = now;
                        }
                    }
                }
            }
        });

        // Task B: OS Thread that blocks waiting for encoded audio data from appsink and broadcasts it.
        // Using standard thread for blocking GStreamer call to avoid blocking the Tokio worker pool.
        let ogg_page_index_clone = ogg_page_index.clone();
        std::thread::spawn(move || {
            loop {
                match appsink.pull_sample() {
                    Ok(sample) => {
                        let sample: gst::Sample = sample;
                        if let Some(buffer) = sample.buffer() {
                            let buffer: &gst::BufferRef = buffer;
                            if let Ok(map) = buffer.map_readable() {
                                let bytes = Bytes::copy_from_slice(map.as_slice());
                                ogg_page_index_clone.ingest(&bytes);

                                // Cache GStreamer header packets (marked with HEADER flag)
                                if buffer.flags().contains(gst::BufferFlags::HEADER) {
                                    let mut headers = stream_headers_clone.lock().unwrap();
                                    headers.push(bytes.clone());
                                    println!("[StreamManager] Cached OGG/stream header buffer of size {}", bytes.len());
                                }

                                // Broadcast to all active HTTP subscribers
                                let _ = tx_mp3_clone.send(bytes);
                            }
                        }
                    }
                    Err(_) => {
                        println!("[StreamManager] Appsink pulled error or EOS. Exiting pull loop.");
                        break;
                    }
                }
            }
        });

        Ok(Self {
            tx_pcm,
            tx_mp3,
            stream_headers,
            playback_timeline,
            ogg_page_index,
        })
    }
}

/// Custom librespot audio Sink that pipes decoded samples into StreamManager's GStreamer pipeline.
pub struct GstreamerStreamingSink {
    tx_pcm: PcmSender,
    playback_timeline: PlaybackTimeline,
    start_time: std::time::Instant,
    samples_written: usize,
}

impl GstreamerStreamingSink {
    pub fn new(tx_pcm: PcmSender, playback_timeline: PlaybackTimeline) -> Self {
        Self {
            tx_pcm,
            playback_timeline,
            start_time: std::time::Instant::now(),
            samples_written: 0,
        }
    }
}

impl Sink for GstreamerStreamingSink {
    fn start(&mut self) -> SinkResult<()> {
        // A stopped player can spend an arbitrary amount of wall time loading its replacement.
        // Restart the pacing epoch so the decoder cannot burst that elapsed time into the PCM and
        // broadcast queues when playback resumes.
        self.start_time = std::time::Instant::now();
        self.samples_written = 0;
        if self.playback_timeline.arm_track_start_after_restart() {
            let discarded_frames = self.tx_pcm.discard_queued();
            if discarded_frames > 0 {
                println!(
                    "[StreamManager] Discarded {:.3}s of stale PCM at track replacement.",
                    discarded_frames as f64 / SAMPLE_RATE as f64
                );
            }
        }
        Ok(())
    }

    fn write(&mut self, packet: AudioPacket, converter: &mut Converter) -> SinkResult<()> {
        let samples_s16 = match packet {
            AudioPacket::Samples(samples) => converter.f64_to_s16(&samples),
            AudioPacket::Raw(_) => {
                return Err(SinkError::OnWrite(
                    "received encoded audio in the PCM/GStreamer sink".to_string(),
                ));
            }
        };

        let num_samples = samples_s16.len();
        if num_samples > 0 {
            // Send the samples to StreamManager
            let track_start_id = self.playback_timeline.take_pending_track_start();
            let _ = self.tx_pcm.send_with_marker(samples_s16, track_start_id);

            // Bounded rate limit to exactly real-time speed based on stereo frames (44.1kHz)
            self.samples_written += num_samples / 2; // stereo frames
            let expected_elapsed =
                std::time::Duration::from_secs_f64(self.samples_written as f64 / 44100.0);
            let target_time = self.start_time + expected_elapsed;
            let now = std::time::Instant::now();
            if now < target_time {
                std::thread::sleep(target_time - now);
            }
        }
        Ok(())
    }
}

/// Librespot passthrough sink that broadcasts the original Ogg/Vorbis pages.
pub struct PassthroughOggStreamingSink {
    tx_mp3: broadcast::Sender<Bytes>,
    stream_headers: Arc<Mutex<Vec<Bytes>>>,
    playback_timeline: PlaybackTimeline,
    ogg_page_index: OggPageIndex,
    total_samples: Arc<Mutex<u64>>,
    parse_buffer: Vec<u8>,
    header_pages: Vec<Bytes>,
    current_serial: Option<u32>,
    stream_start: std::time::Instant,
    last_granule: u64,
}

impl PassthroughOggStreamingSink {
    pub fn new(
        tx_mp3: broadcast::Sender<Bytes>,
        stream_headers: Arc<Mutex<Vec<Bytes>>>,
        playback_timeline: PlaybackTimeline,
        ogg_page_index: OggPageIndex,
        total_samples: Arc<Mutex<u64>>,
    ) -> Self {
        Self {
            tx_mp3,
            stream_headers,
            playback_timeline,
            ogg_page_index,
            total_samples,
            parse_buffer: Vec::new(),
            header_pages: Vec::new(),
            current_serial: None,
            stream_start: std::time::Instant::now(),
            last_granule: 0,
        }
    }

    fn ingest_page_metadata(&mut self, raw_bytes: &[u8]) -> Option<u64> {
        self.ogg_page_index.ingest(raw_bytes);
        self.parse_buffer.extend_from_slice(raw_bytes);
        let pages = parse_ogg_pages(&mut self.parse_buffer);
        let mut latest_granule = None;

        for page in pages {
            let is_bos = (page.header_type & 0x02) != 0;
            if is_bos || self.current_serial != Some(page.serial) {
                self.current_serial = Some(page.serial);
                self.header_pages.clear();
                self.stream_start = std::time::Instant::now();
                self.last_granule = 0;
                *self.total_samples.lock().unwrap() = 0;

                if let Some(track_id) = self.playback_timeline.take_pending_track_start() {
                    self.playback_timeline.record_track_start(track_id, 0.0);
                }
            }

            match page.granule_pos {
                Some(0) => {
                    self.header_pages.push(page.bytes.clone());
                    *self.stream_headers.lock().unwrap() = self.header_pages.clone();
                }
                Some(granule) => {
                    if !self.header_pages.is_empty() {
                        *self.stream_headers.lock().unwrap() = self.header_pages.clone();
                    }
                    latest_granule = Some(granule);
                    self.last_granule = granule;
                    *self.total_samples.lock().unwrap() = granule;
                }
                None => {}
            }
        }

        latest_granule
    }

    fn pace_after_send(&self, granule: u64) {
        let expected_elapsed =
            std::time::Duration::from_secs_f64(granule as f64 / VORBIS_GRANULE_RATE);
        let target_time = self.stream_start + expected_elapsed;
        let now = std::time::Instant::now();
        if now < target_time {
            std::thread::sleep(target_time - now);
        }
    }
}

impl Sink for PassthroughOggStreamingSink {
    fn start(&mut self) -> SinkResult<()> {
        self.playback_timeline.arm_track_start_after_restart();
        Ok(())
    }

    fn write(&mut self, packet: AudioPacket, _converter: &mut Converter) -> SinkResult<()> {
        let raw_bytes = match packet {
            AudioPacket::Raw(raw_bytes) => raw_bytes,
            AudioPacket::Samples(_) => {
                return Err(SinkError::OnWrite(
                    "received decoded samples in the Ogg passthrough sink".to_string(),
                ));
            }
        };

        if raw_bytes.is_empty() {
            return Ok(());
        }

        let latest_granule = self.ingest_page_metadata(&raw_bytes);
        let _ = self.tx_mp3.send(Bytes::from(raw_bytes));

        if let Some(granule) = latest_granule {
            self.pace_after_send(granule);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restart_track_markers_cannot_be_consumed_by_old_audio() {
        let timeline = PlaybackTimeline::default();
        timeline.mark_next_buffer_after_restart_as_track_start("new-track".to_string());

        assert_eq!(timeline.take_pending_track_start(), None);
        assert!(timeline.arm_track_start_after_restart());
        assert_eq!(
            timeline.take_pending_track_start().as_deref(),
            Some("new-track")
        );
    }

    #[test]
    fn replacing_a_track_discards_stale_pcm_when_the_sink_restarts() {
        let (sender, _receiver) = bounded_pcm_channel();
        sender
            .send_with_marker(vec![0; CHANNELS * 128], None)
            .unwrap();
        let timeline = PlaybackTimeline::default();
        timeline.mark_next_buffer_after_restart_as_track_start("new-track".to_string());
        let mut sink = GstreamerStreamingSink::new(sender, timeline);

        Sink::start(&mut sink).unwrap();

        assert_eq!(sink.tx_pcm.shared.queue.lock().unwrap().queued_frames, 0);
    }

    #[test]
    fn starting_the_encoded_sink_resets_its_realtime_pacer() {
        let (sender, _receiver) = bounded_pcm_channel();
        let mut sink = GstreamerStreamingSink::new(sender, PlaybackTimeline::default());
        sink.start_time = Instant::now() - Duration::from_secs(10);
        sink.samples_written = SAMPLE_RATE * 10;
        let restarted_after = Instant::now();

        Sink::start(&mut sink).unwrap();

        assert!(sink.start_time >= restarted_after);
        assert_eq!(sink.samples_written, 0);
    }
}
