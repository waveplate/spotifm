// Public demo playback uses only the songs and timing exported by the builder.
(() => {
    'use strict';
    const catalog = window.SpotifmDemoMedia;
    const { parseLrc, formatTime } = catalog;

    async function loadLyricsBundle(config) {
        const request = new AbortController();
        const timeout = window.setTimeout(() => request.abort(), 15000);
        try {
            const response = await fetch(config.lyricsUrl, { credentials: 'omit', cache: 'no-cache', signal: request.signal });
            if (!response.ok) throw new Error(`Bundled lyrics returned HTTP ${response.status}`);
            const records = (await response.text()).split(/\r?\n/).filter(line => line.trim()).map(line => JSON.parse(line));
            const manifest = records.shift();
            if (manifest?.type !== 'manifest' || manifest.version !== 1 ||
                manifest.tracks_sha256 !== config.lyricsTracksHash || manifest.track_count !== config.tracks.length) {
                throw new Error('The lyric bundle does not match this demo');
            }
            const byIndex = new Map();
            for (const record of records) {
                const index = record.track_index;
                if (!Number.isInteger(index) || index < 0 || index >= config.tracks.length || byIndex.has(index) ||
                    record.source !== 'lrclib' || !Number.isSafeInteger(record.lrclib_id) || record.lrclib_id <= 0 ||
                    (config.tracks[index].lrclibId && config.tracks[index].lrclibId !== record.lrclib_id)) {
                    throw new Error('The lyric bundle contains an invalid recording');
                }
                const bytes = Uint8Array.from(atob(record.lyrics_base64), character => character.charCodeAt(0));
                const lrc = new TextDecoder('utf-8', { fatal: true }).decode(bytes);
                const lines = parseLrc(lrc);
                if (!lines.some(line => line.text)) throw new Error('The lyric bundle contains no timed lyrics');
                byIndex.set(index, { id: record.lrclib_id, lines });
            }
            if (byIndex.size !== config.tracks.length) throw new Error('The lyric bundle is missing recordings');
            return byIndex;
        } finally { window.clearTimeout(timeout); }
    }

    class DemoSource {
        constructor(audio, config, callbacks) {
            this.audio = audio;
            this.tracks = Array.isArray(config.tracks) ? config.tracks.map(track => ({ ...track })) : [];
            this.callbacks = callbacks;
            this.index = -1;
            this.lines = [];
            this.generation = 0;
            this.lyricsState = 'loading';
            // One request to a static file, shared across every song change.
            this.lyricsBundleReady = loadLyricsBundle(config);
            void this.lyricsBundleReady.catch(() => {});
            this.mountControls();
            audio.loop = false;
            audio.addEventListener('ended', () => {
                if (audio.ended && this.track) this.step(1, true);
            });
            audio.addEventListener('loadedmetadata', () => {
                this.seek.disabled = false;
                this.seek.max = String(this.duration);
                callbacks.onDuration(this.duration * 1000);
                this.updateClock();
                if (this.lines.length) this.emitLyrics();
            });
            audio.addEventListener('timeupdate', () => this.updateClock());
            audio.addEventListener('seeking', () => callbacks.onSeek());
            audio.addEventListener('seeked', () => callbacks.onSeek());
            audio.addEventListener('error', () => {
                if (!audio.error || !this.track) return;
                this.audioFailed = true;
                this.setStatus('This preview is unavailable. Try another sample.', true);
                callbacks.onAudioError();
            });
        }

        get track() { return this.tracks[this.index]; }
        get trackId() {
            return this.track?.spotifyId || (this.track?.appleSongId ? `apple:${this.track.appleSongId}` : `demo:${this.index}`);
        }
        get duration() {
            return Number.isFinite(this.audio.duration) && this.audio.duration > 0 ? this.audio.duration : 30;
        }

        mountControls() {
            const panel = document.createElement('section');
            panel.className = 'demo-panel';
            panel.setAttribute('aria-label', 'Player demo');
            panel.innerHTML = `
                <div class="demo-heading"><span>SPOTIFM DEMO</span></div>
                <div class="demo-controls">
                    <label class="demo-track-label">Sample <select aria-label="Choose a demo sample"></select></label>
                    <button type="button" class="demo-previous" aria-keyshortcuts="F5">Previous</button>
                    <button type="button" class="demo-replay">Replay</button>
                    <button type="button" class="demo-next" aria-keyshortcuts="F7">Next</button>
                </div>
                <div class="demo-seek-row">
                    <input class="demo-seek" type="range" min="0" max="30" step="0.05" value="0" aria-label="Preview playback position" disabled>
                    <output class="demo-clock">0:00 / 0:30</output>
                </div>
                <div class="demo-caption"><span class="demo-status" role="status" aria-live="polite">Choose a sample and click to play.</span>
                    <span class="demo-sources"><a class="demo-audio-link" target="_blank" rel="noopener noreferrer">Audio preview</a> · <a class="demo-lyrics-link" href="https://lrclib.net" target="_blank" rel="noopener noreferrer">LRCLIB lyrics</a></span>
                </div>
                <div class="demo-settings-hint" role="note">Open settings with the gear icon at the top right of the page.</div>`;
            document.body.appendChild(panel);
            this.select = panel.querySelector('select');
            this.seek = panel.querySelector('.demo-seek');
            this.clock = panel.querySelector('.demo-clock');
            this.status = panel.querySelector('.demo-status');
            this.audioLink = panel.querySelector('.demo-audio-link');
            this.lyricsLink = panel.querySelector('.demo-lyrics-link');
            this.tracks.forEach((track, index) => {
                const option = document.createElement('option');
                option.value = String(index);
                option.textContent = `${track.title} — ${(track.artists || [track.artist]).join(', ')}`;
                this.select.appendChild(option);
            });
            this.select.addEventListener('change', () => this.selectTrack(Number(this.select.value), true));
            panel.querySelector('.demo-replay').addEventListener('click', () => {
                if (!this.track) return;
                this.audio.currentTime = 0;
                this.callbacks.onSeek();
                this.callbacks.onPlay();
            });
            const next = panel.querySelector('.demo-next');
            const previous = panel.querySelector('.demo-previous');
            next.disabled = this.tracks.length < 2;
            previous.disabled = this.tracks.length < 2;
            next.addEventListener('click', () => this.step(1, true));
            previous.addEventListener('click', () => this.step(-1, true));
            this.seek.addEventListener('input', () => {
                this.audio.currentTime = Number(this.seek.value);
                this.updateClock();
                this.callbacks.onSeek();
            });
        }

        setStatus(message, error = false) {
            if (this.audioFailed) {
                message = 'This preview is unavailable. Try another sample.';
                error = true;
            }
            this.status.textContent = message;
            this.status.classList.toggle('demo-error', error);
        }

        updateClock() {
            const time = Number.isFinite(this.audio.currentTime) ? this.audio.currentTime : 0;
            this.seek.value = String(time);
            this.clock.textContent = `${formatTime(time)} / ${formatTime(this.duration)}`;
        }

        step(direction, play) {
            if (this.tracks.length) this.selectTrack((this.index + direction + this.tracks.length) % this.tracks.length, play);
        }

        selectTrack(index, play = false) {
            const track = this.tracks[index];
            if (!track) { this.setStatus('No demo samples configured.', true); return; }
            this.generation += 1;
            this.callbacks.onPause();
            this.index = index;
            this.select.value = String(index);
            this.lines = [];
            this.audioFailed = false;
            this.lyricsState = 'loading';
            this.seek.disabled = true;
            this.seek.value = '0';
            this.lyricsLink.href = 'https://lrclib.net';
            // A signed start is full-song time at audio time zero. A negative
            // value delays the first lyrics without seeking audio below zero.
            this.previewStartMs = Number.isFinite(track.previewStartMs) ? track.previewStartMs : 0;
            this.timingKnown = Number.isFinite(track.previewStartMs);
            this.updateAudioLink(track);
            this.setStatus('Loading preview and lyrics…');
            this.audio.removeAttribute('src');
            this.audio.load();
            this.callbacks.onTrack(track);
            this.updateClock();
            void this.loadLyrics(track, this.generation);
            this.previewReady = this.preparePreview(track, this.generation);
            if (play) this.callbacks.onPlay();
        }

        updateAudioLink(track) {
            this.audioLink.href = track.appleSongId ? `https://music.apple.com/us/song/${track.appleSongId}`
                : track.spotifyId ? `https://open.spotify.com/track/${encodeURIComponent(track.spotifyId)}` : track.previewUrl;
            this.audioLink.textContent = track.appleSongId ? 'Apple Music preview' : 'Audio preview';
        }

        async preparePreview(track, generation) {
            try {
                if (!track.previewUrl) {
                    const song = await catalog.lookupAppleTrack(track);
                    if (generation !== this.generation) return false;
                    catalog.applyAppleMetadata(track, song);
                    this.updateAudioLink(track);
                    this.callbacks.onTrack(track, true);
                }
                if (generation !== this.generation) return false;
                this.audio.src = track.previewUrl;
                if (this.lyricsState === 'loading') this.setStatus('Loading synced lyrics…');
                return true;
            } catch (error) {
                if (generation !== this.generation) return false;
                console.warn('[Demo] Could not load preview:', error);
                this.audioFailed = true;
                this.setStatus('This preview is unavailable. Try another sample.', true);
                this.callbacks.onAudioError();
                return false;
            }
        }

        async loadLyrics(track, generation) {
            this.lyricsState = 'loading';
            const index = this.index;
            try {
                const result = (await this.lyricsBundleReady).get(index);
                if (generation !== this.generation) return;
                this.lines = result.lines;
                this.lyricsState = 'ready';
                this.lyricsLink.href = `https://lrclib.net/api/get/${result.id}`;
                this.emitLyrics();
            } catch (error) {
                if (generation !== this.generation) return;
                console.warn('[Demo] Could not load bundled lyrics:', error);
                this.lyricsState = 'unavailable';
                this.setStatus(window.location.protocol === 'file:'
                    ? 'Open this demo through a web server to load its bundled lyrics.'
                    : 'Bundled lyrics could not be loaded. Audio and visuals are still available.', true);
                this.callbacks.onLyrics(null);
            }
        }

        emitLyrics() {
            const start = this.previewStartMs;
            const end = start + this.duration * 1000;
            const preceding = this.lines.filter(line => line.time_ms <= start).at(-1);
            const lines = this.lines.filter(line => line.time_ms > start && line.time_ms < end)
                .map(line => ({ ...line, time_ms: line.time_ms - start }));
            if (preceding) lines.unshift({ ...preceding, time_ms: 0 });
            this.setStatus(this.timingKnown ? 'The next sample plays automatically when this clip ends.'
                : 'Lyric timing has not been set for this sample.');
            this.callbacks.onLyrics(lines);
        }
    }

    window.SpotifmDemoSource = DemoSource;
})();
